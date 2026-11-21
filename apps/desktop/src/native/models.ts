import { threadSupportsProviderHandoff } from "@t3tools/client-runtime/state/thread-workflows";
import { boundedSnapshotProjection } from "@t3tools/shared/orchestrationV2BoundedSnapshot";
import { PermissionUnavailable, permissionsForProvider } from "./permissions.ts";
import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  isProviderInstancePickerReady,
  providerInstanceEntriesFor,
  type ProviderInstanceEntry,
} from "@t3tools/client-runtime/state/provider-instances";
import {
  CommandId,
  ORCHESTRATION_V2_WS_METHODS,
  WS_METHODS,
  type ServerSettings,
  type ServerConfig,
  type OrchestrationV2ThreadProjection,
} from "@t3tools/contracts";
import { resolveProjectSettings } from "@t3tools/shared/projectSettings";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { readThreadSettingsSnapshot, type Session } from "./projects.ts";

export class ModelUnavailable extends Schema.TaggedError<ModelUnavailable>()("ModelUnavailable", {
  message: Schema.String,
}) {}

export function availableModels(
  entry: ProviderInstanceEntry,
  settings: ServerSettings,
  root?: string,
) {
  const unavailable = new Set(
    entry.snapshot.subProviders?.filter((sub) => sub.unavailableReason).map((sub) => sub.name),
  );
  const hidden = new Set(settings.providerModelPreferences?.[entry.instanceId]?.hiddenModels);
  const catalog =
    entry.snapshot.workspaceSnapshots?.find((workspace) => workspace.cwd === root)?.models ??
    entry.models;
  return catalog.filter(
    (model) =>
      (model.isCustom || !hidden.has(model.slug)) &&
      (!model.subProvider || !unavailable.has(model.subProvider)),
  );
}

type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;

type CatalogThread = Pick<
  OrchestrationV2ThreadProjection["thread"],
  "modelSelection" | "projectId" | "worktreePath"
>;

const readModelCatalog = Effect.fn("nativeDesktop.readModelCatalog")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  projectId: string,
  instance?: string,
) {
  const config = yield* rpc[WS_METHODS.serverGetConfig]({});
  const scoped = config.providers.some(
    (p) =>
      (!instance || p.instanceId === instance) &&
      p.workspaceSnapshots?.some((w) => w.models !== undefined),
  );
  if (!scoped) return { config, root: undefined };
  const projects = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "projects");
  const { projects: records } = yield* projects.snapshot({
    headers: { authorization: `Bearer ${session.token}` },
  });
  const root = records.find((p) => p.id === projectId && p.deletedAt === null)?.workspaceRoot;
  return { config, root };
});

function threadModels(
  config: ServerConfig,
  thread: CatalogThread,
  root: string | undefined,
  instance?: string,
) {
  const settings = resolveProjectSettings(config.settings, thread.projectId).settings;
  return providerInstanceEntriesFor(config.providers, config.settings, thread.projectId)
    .filter(
      (entry) =>
        (!instance || entry.instanceId === instance) && isProviderInstancePickerReady(entry),
    )
    .flatMap((entry) =>
      availableModels(entry, settings, thread.worktreePath ?? root).map((m) => ({
        instance: entry.instanceId,
        model: m.slug,
        name: m.name,
        provider: entry.displayName,
      })),
    );
}

export const readThreadModels = Effect.fn("nativeDesktop.readThreadModels")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  projection: OrchestrationV2ThreadProjection,
) {
  const { thread } = projection;
  const instance = threadSupportsProviderHandoff(projection)
    ? undefined
    : thread.modelSelection.instanceId;
  const { config, root } = yield* readModelCatalog(session, rpc, thread.projectId, instance);
  return threadModels(config, thread, root, instance);
});

export type Handoff = { project: string; source: string; instance: string; model: string };
type Permission = { instance: string; mode: string };

export const selectionForSend = Effect.fn("nativeDesktop.selectionForSend")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  input: {
    thread: string;
    operation: string;
    permission?: Permission | null;
    model?: Handoff | null;
  },
) {
  const { permission, model } = input;
  if (!permission && !model) return undefined;
  const target = model?.instance ?? permission!.instance;
  const { config, root } = model
    ? yield* readModelCatalog(session, rpc, model.project, target)
    : { config: yield* rpc[WS_METHODS.serverGetConfig]({}), root: undefined };
  const modes = yield* permissionsForProvider(config, target);
  const snapshot = yield* readThreadSettingsSnapshot(session, input.thread);
  const validate = (state: typeof snapshot) => {
    const thread = state.projection.thread;
    if (
      thread.deletedAt ||
      thread.archivedAt ||
      thread.modelSelection.instanceId !== (model?.source ?? target)
    )
      return null;
    if (!model) return thread.modelSelection;
    if (
      thread.projectId !== model.project ||
      !threadSupportsProviderHandoff(boundedSnapshotProjection(state))
    )
      return null;
    const choice = threadModels(config, thread, root, target).find((c) => c.model === model.model);
    return choice && { instanceId: choice.instance, model: choice.model };
  };
  const selection = validate(snapshot);
  if (!selection)
    return yield* new ModelUnavailable({
      message:
        "This thread or its provider changed. Review the model and permissions before sending.",
    });
  const mode = modes.find(
    (m) => m.mode === (permission?.mode ?? snapshot.projection.thread.runtimeMode),
  );
  if ((permission && permission.instance !== target) || !mode || mode.denied)
    return yield* new PermissionUnavailable({
      message:
        mode?.denied ??
        "Choose a permission level supported by the selected provider before sending.",
    });
  if (mode.mode === snapshot.projection.thread.runtimeMode) return selection;
  // Existing servers persist the shared thread mode separately from dispatch, as in the web composer.
  yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "thread.runtime-mode.set",
    commandId: CommandId.make(input.operation + ":permissions:" + snapshot.snapshotSequence),
    threadId: snapshot.projection.thread.id,
    runtimeMode: mode.mode,
  });
  const current = yield* readThreadSettingsSnapshot(session, input.thread);
  const latest = validate(current);
  if (!latest || current.projection.thread.runtimeMode !== mode.mode)
    return yield* new PermissionUnavailable({
      message:
        "This thread changed while saving permissions. Review the current selection before sending.",
    });
  return latest;
});

export const setThreadModel = Effect.fn("nativeDesktop.setThreadModel")(function* (
  session: Session,
  request: { thread: string; instance: string; model: string; operation: string },
  watched: OrchestrationV2ThreadProjection["thread"],
) {
  if (watched.id !== request.thread || request.instance !== watched.modelSelection.instanceId)
    return yield* new ModelUnavailable({
      message: "This thread’s provider changed. Reopen the model picker.",
    });
  const rpc = yield* openEnvironmentRpc(session);
  const { config, root } = yield* readModelCatalog(
    session,
    rpc,
    watched.projectId,
    request.instance,
  );
  const current = (yield* readThreadSettingsSnapshot(session, request.thread)).projection.thread;
  if (
    current.archivedAt ||
    current.deletedAt ||
    current.modelSelection.instanceId !== request.instance ||
    current.projectId !== watched.projectId
  )
    return yield* new ModelUnavailable({
      message: "This thread changed. Reopen the model picker.",
    });
  if (current.modelSelection.model === request.model) return;
  const options = threadModels(config, current, root, request.instance);
  if (!options.some((o) => o.model === request.model))
    return yield* new ModelUnavailable({
      message: "This model is no longer available. Reopen the picker to choose another.",
    });
  yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "thread.model-selection.set",
    threadId: current.id,
    commandId: CommandId.make(request.operation),
    modelSelection: { instanceId: current.modelSelection.instanceId, model: request.model },
  });
});
