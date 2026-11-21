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
  thread: CatalogThread,
) {
  const config = yield* rpc[WS_METHODS.serverGetConfig]({});
  const scoped = config.providers
    .find((p) => p.instanceId === thread.modelSelection.instanceId)
    ?.workspaceSnapshots?.some((w) => w.models !== undefined);
  if (!scoped) return { config, root: undefined };
  const projects = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "projects");
  const { projects: records } = yield* projects.snapshot({
    headers: { authorization: `Bearer ${session.token}` },
  });
  const root = records.find(
    (p) => p.id === thread.projectId && p.deletedAt === null,
  )?.workspaceRoot;
  return { config, root };
});

const threadModels = Effect.fn("nativeDesktop.threadModels")(function* (
  config: ServerConfig,
  thread: CatalogThread,
  root?: string,
) {
  const entry = providerInstanceEntriesFor(
    config.providers,
    config.settings,
    thread.projectId,
  ).find((p) => p.instanceId === thread.modelSelection.instanceId);
  if (!entry || !isProviderInstancePickerReady(entry))
    return yield* new ModelUnavailable({
      message: "This provider’s model catalog is unavailable. Reopen the picker to retry.",
    });
  const settings = resolveProjectSettings(config.settings, thread.projectId).settings;
  return availableModels(entry, settings, thread.worktreePath ?? root).map((m) => ({
    id: m.slug,
    name: m.name,
    provider: entry.displayName,
  }));
});

export const readThreadModels = Effect.fn("nativeDesktop.readThreadModels")(function* (
  session: Session,
  rpc: EnvironmentRpc,
  thread: CatalogThread,
) {
  const { config, root } = yield* readModelCatalog(session, rpc, thread);
  return yield* threadModels(config, thread, root);
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
  const { config, root } = yield* readModelCatalog(session, rpc, watched);
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
  const options = yield* threadModels(config, current, root);
  if (!options.some((o) => o.id === request.model))
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
