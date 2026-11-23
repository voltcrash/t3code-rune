import {
  BearerConnectionTarget,
  type PreparedConnection,
} from "@t3tools/client-runtime/connection";
import {
  cancelEnvironmentSpeechModelDownload,
  downloadEnvironmentSpeechModel,
  getEnvironmentSpeechModels,
  getEnvironmentSpeechStatus,
  removeEnvironmentSpeechModel,
  selectEnvironmentSpeechModel,
} from "@t3tools/client-runtime/voice-input";
import type { EnvironmentSpeechModel } from "@t3tools/contracts";
import { formatBytes } from "@t3tools/shared/formatBytes";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
import type { Session } from "./projects.ts";

type SpeechSession = Pick<
  Session,
  "environmentId" | "label" | "httpBaseUrl" | "token" | "canWriteSettings" | "http"
>;

export function speechConnection(session: SpeechSession): PreparedConnection {
  const { environmentId, label, httpBaseUrl, token } = session;
  const socket = new URL("/ws", httpBaseUrl);
  socket.protocol = socket.protocol === "https:" ? "wss:" : "ws:";
  return {
    environmentId,
    label,
    httpBaseUrl,
    socketUrl: socket.href,
    target: new BearerConnectionTarget({ environmentId, label, connectionId: environmentId }),
    httpAuthorization: { _tag: "Bearer", token },
  };
}

const operations = {
  download: downloadEnvironmentSpeechModel,
  select: selectEnvironmentSpeechModel,
  cancel: cancelEnvironmentSpeechModelDownload,
  remove: removeEnvironmentSpeechModel,
};
type Operation = keyof typeof operations;

function transferring(model: EnvironmentSpeechModel) {
  return model.state === "downloading" || model.state === "verifying";
}

function allowed(model: EnvironmentSpeechModel, action: Operation) {
  switch (action) {
    case "download":
      return model.state === "downloadable";
    case "select":
      return model.state === "installed" && !model.active;
    case "remove":
      return model.state === "installed";
    case "cancel":
      return transferring(model);
  }
}

function row(model: EnvironmentSpeechModel) {
  const state = model.state;
  return {
    id: model.id,
    name: model.name,
    size: formatBytes(model.size),
    state,
    active: model.active,
    progress:
      state === "verifying"
        ? m(msg("Verifying download…"))
        : state === "downloading"
          ? m(
              msg("{percent}% downloaded", {
                percent: Math.round((100 * (model.downloaded ?? 0)) / Math.max(1, model.size)),
              }),
            )
          : null,
    remove_label: model.linkedFrom
      ? m(msg("Stop using your {name} file; the file stays on disk", { name: model.name }))
      : m(msg("Delete {name}", { name: model.name })),
  };
}

type View = {
  models: ReturnType<typeof row>[];
  message: string | null;
  busy: boolean;
  can_cancel: boolean;
  manage: boolean;
};

export function watchVoiceModels(
  session: SpeechSession,
  signal: AbortSignal,
  publish: (view: View) => void,
) {
  const connection = speechConnection(session);
  let models: readonly EnvironmentSpeechModel[] = [];
  let visible = false;
  let progress: AbortController | undefined;
  let reading: AbortController | undefined;
  let actionError: string | null = null;
  let nextMutation = 0;
  let appliedMutation = 0;
  let busy = false;
  let cancelling = false;
  let download = false;
  let message: string | null = null;

  function emit() {
    if (!visible || signal.aborted) return;
    publish({
      models: models.map(row),
      message: actionError ?? message,
      manage: session.canWriteSettings,
      busy: busy || cancelling || models.some(transferring),
      can_cancel: !cancelling && (!busy || download),
    });
  }

  function scheduleProgress() {
    progress?.abort();
    if (!visible || signal.aborted || (!download && !models.some(transferring))) return;
    progress = new AbortController();
    const waiting = AbortSignal.any([signal, progress.signal]);
    void Effect.runPromise(Effect.sleep("1 second"), { signal: waiting }).then(refresh, () => {});
  }

  async function refresh() {
    progress?.abort();
    reading?.abort();
    if (!visible || signal.aborted) return;
    reading = new AbortController();
    const read = AbortSignal.any([reading.signal, signal]);
    try {
      const catalog = await Effect.runPromise(
        Effect.gen(function* () {
          const status = yield* getEnvironmentSpeechStatus(connection);
          if (!status.supported)
            return {
              models: [],
              message: m(
                msg("This environment can’t transcribe audio: {reason}.", {
                  reason: status.reason,
                }),
              ),
            };
          const result = yield* getEnvironmentSpeechModels(connection);
          return { models: result.models, message: null };
        }).pipe(Effect.provide(session.http)),
        { signal: read },
      );
      if (read.aborted) return;
      models = catalog.models;
      message = catalog.message;
    } catch {
      if (read.aborted) return;
      message = m(msg("Could not load transcription models"));
    }
    emit();
    scheduleProgress();
  }

  async function change(action: Operation, id: string) {
    if (
      !visible ||
      signal.aborted ||
      !session.canWriteSettings ||
      !Object.hasOwn(operations, action)
    )
      return;
    const model = models.find((item) => item.id === id);
    if (!model || !allowed(model, action)) return;
    if (action === "cancel") {
      if (cancelling || (busy && !download)) return;
      cancelling = true;
    } else {
      if (busy || cancelling || models.some(transferring)) return;
      busy = true;
      download = action === "download";
    }
    const mutation = ++nextMutation;
    actionError = null;
    emit();
    if (action === "download") scheduleProgress();
    try {
      const result = await Effect.runPromise(
        operations[action](connection, id).pipe(Effect.provide(session.http)),
        { signal },
      );
      if (mutation > appliedMutation) {
        appliedMutation = mutation;
        models = result.models;
        emit();
      }
    } catch {
      actionError = m(
        {
          download: msg("Could not download the model"),
          select: msg("Could not select the model"),
          cancel: msg("Could not cancel the download"),
          remove: msg("Could not delete the model"),
        }[action],
      );
    } finally {
      await refresh();
      if (action === "cancel") cancelling = false;
      else {
        busy = false;
        download = false;
      }
      emit();
      scheduleProgress();
    }
  }

  return {
    show(open: boolean) {
      visible = open;
      actionError = null;
      if (open) return refresh();
      reading?.abort();
      progress?.abort();
    },
    change,
  };
}
