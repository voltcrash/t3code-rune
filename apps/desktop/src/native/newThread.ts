import { deriveThreadTitleSeed } from "@t3tools/client-runtime/operations";
import {
  isProviderInstancePickerReady,
  providerInstanceEntriesFor,
  resolveProjectDefaultProviderModelSelection,
} from "@t3tools/client-runtime/state/provider-instances";
import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  clampManagedRuntimeMode,
  CommandId,
  DEFAULT_PROVIDER_INTERACTION_MODE,
  MessageId,
  ORCHESTRATION_V2_WS_METHODS,
  ProjectId,
  RuntimeMode,
  ThreadId,
  T3_PROJECT_FILE_NAME,
  WS_METHODS,
  type ModelSelection,
  type OrchestrationV2ThreadLaunchInput,
} from "@t3tools/contracts";
import { parseT3ProjectFile } from "@t3tools/shared/t3ProjectFile";
import { resolveNewThreadEnvMode, resolveProjectSettings } from "@t3tools/shared/projectSettings";
import { resolveNewThreadModelSelection } from "@t3tools/shared/usageLimits";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as DateTime from "effect/DateTime";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import * as Option from "effect/Option";
import { prepareAttachments, retryAttachments } from "./attachments.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { headersFor, projectSummary, type Session } from "./projects.ts";

export class NewThreadUnavailable extends Schema.TaggedError<NewThreadUnavailable>()(
  "NewThreadUnavailable",
  { message: Schema.String },
) {}

type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;

const launchContext = Effect.fn("nativeDesktop.launchContext")(function* (
  session: Session,
  project: string,
  rpc: EnvironmentRpc,
) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const [config, shell] = yield* Effect.all(
    [
      rpc[WS_METHODS.serverGetConfig]({}),
      client.shellSnapshot({ headers: headersFor(session), query: { omitIdleSubagents: "1" } }),
    ],
    { concurrency: "unbounded" },
  );
  const projectId = ProjectId.make(project);
  const record = "projects" in shell ? shell.projects.find((p) => p.id === projectId) : undefined;
  if (!record || record.archivedAt)
    return yield* new NewThreadUnavailable({
      message: "This project is unavailable. Refresh the project list.",
    });
  const projectFile =
    session.canBrowse &&
    resolveProjectSettings(config.settings, projectId, record).settings.defaultThreadEnvMode ===
      null
      ? yield* rpc[WS_METHODS.projectsReadFile]({
          cwd: record.workspaceRoot,
          relativePath: T3_PROJECT_FILE_NAME,
          allowMissing: true,
        }).pipe(
          Effect.map((file) =>
            file.exists === false || file.truncated ? null : parseT3ProjectFile(file.contents),
          ),
          Effect.orElseSucceed(() => null),
        )
      : null;
  const projectSettings = resolveProjectSettings(config.settings, projectId, record, projectFile);
  const { settings } = projectSettings;
  const { selection: preferred } = resolveNewThreadModelSelection({
    defaultSelection:
      settings.defaultModelSelection ??
      resolveProjectDefaultProviderModelSelection(config.providers, config.settings, projectId),
    fallbackSelection: settings.fallbackModelSelection,
    providers: config.providers,
    settings: config.settings,
    projectId,
    now: yield* Clock.currentTimeMillis,
  });
  const entries = providerInstanceEntriesFor(config.providers, config.settings, projectId);
  const choices = entries.filter(isProviderInstancePickerReady).flatMap((entry) => {
    const supported = entry.snapshot.supportedRuntimeModes;
    const modes = RuntimeMode.literals.filter(
      (mode) => !supported?.length || supported.includes(mode),
    );
    const mode = modes.includes(settings.defaultRuntimeMode)
      ? settings.defaultRuntimeMode
      : modes[0];
    const runtime = mode && clampManagedRuntimeMode(config.managedPolicy, mode, modes);
    if (!runtime) return [];
    const unavailable = new Set(
      entry.snapshot.subProviders?.filter((s) => s.unavailableReason).map((s) => s.name),
    );
    const hidden = new Set(settings.providerModelPreferences?.[entry.instanceId]?.hiddenModels);
    const catalog =
      entry.snapshot.workspaceSnapshots?.find((s) => s.cwd === record.workspaceRoot)?.models ??
      entry.models;
    return catalog
      .filter((model) => model.isCustom || !hidden.has(model.slug))
      .filter((m) => !m.subProvider || !unavailable.has(m.subProvider))
      .map((model) => {
        const isPreferred =
          preferred?.instanceId === entry.instanceId && preferred.model === model.slug;
        const selection: ModelSelection = isPreferred
          ? preferred
          : { instanceId: entry.instanceId, model: model.slug };
        return {
          id: JSON.stringify([entry.instanceId, model.slug]),
          name: model.name,
          provider: entry.displayName,
          runtime,
          selection,
          preferred: isPreferred,
        };
      });
  });
  return {
    record: projectSummary(record, config.scratchWorkspaceRoot),
    cwd: record.workspaceRoot,
    startFromOrigin: settings.newWorktreesStartFromOrigin,
    project,
    choices,
    worktree:
      resolveNewThreadEnvMode({
        projectSettings,
        workspaceRoot: record.workspaceRoot,
        newProjectsRoot: config.newProjectsRoot,
        projectHasThreads:
          "threads" in shell &&
          shell.threads.some((t) => t.projectId === projectId && t.deletedAt === null),
      }) === "worktree",
  };
});

export const readLaunchOptions = Effect.fn("nativeDesktop.readLaunchOptions")(function* (
  session: Session,
  existingProject: string | null,
) {
  const rpc = yield* openEnvironmentRpc(session);
  const project = existingProject ?? (yield* rpc[WS_METHODS.projectsEnsureScratch]({})).projectId;
  const options = yield* launchContext(session, project, rpc);
  const branch = options.record.scratch
    ? null
    : yield* rpc[WS_METHODS.subscribeVcsBranch]({ cwd: options.cwd }).pipe(
        Stream.runHead,
        Effect.map(Option.getOrNull),
        Effect.orElseSucceed(() => null),
      );
  return {
    project,
    record: options.record,
    worktree: !options.record.scratch && options.worktree,
    base: branch?.refName ?? (options.startFromOrigin ? "" : "HEAD"),
    choices: options.choices.map(({ id, name, provider, runtime, preferred }) => ({
      id,
      name,
      provider,
      runtime,
      preferred,
    })),
  };
});

const prepareLaunch = Effect.fn("nativeDesktop.prepareLaunch")(function* (
  session: Session,
  request: {
    project: string;
    model: string;
    text: string;
    files: readonly string[];
    operation: string;
    runtime: string;
    base: string | null;
  },
  rpc: EnvironmentRpc,
) {
  const options = yield* launchContext(session, request.project, rpc);
  const choice = options.choices.find((c) => c.id === request.model);
  if (!choice)
    return yield* new NewThreadUnavailable({
      message:
        "This model is unavailable for this project. Reopen the model picker to refresh its choices.",
    });
  if (choice.runtime !== request.runtime)
    return yield* new NewThreadUnavailable({
      message:
        "The project’s permission settings changed. Refresh and review the permission level before starting.",
    });
  const attachments = yield* prepareAttachments(
    session,
    rpc,
    request.files,
    request.operation,
    request.operation,
  );
  return {
    threadId: ThreadId.make(request.operation),
    commandId: CommandId.make(request.operation),
    projectId: ProjectId.make(request.project),
    title: deriveThreadTitleSeed({ text: request.text, attachments }),
    creationSource: "web",
    modelSelection: choice.selection,
    runtimeMode: choice.runtime,
    interactionMode: DEFAULT_PROVIDER_INTERACTION_MODE,
    workspaceStrategy:
      request.base === null
        ? { type: "root" }
        : { type: "worktree", baseRef: request.base, startFromOrigin: options.startFromOrigin },
    initialMessage: {
      messageId: MessageId.make(request.operation),
      text: request.text,
      attachments,
    },
  } satisfies OrchestrationV2ThreadLaunchInput;
});

export const launchThread = Effect.fn("nativeDesktop.launchThread")(function* (
  session: Session,
  request: Parameters<typeof prepareLaunch>[1],
  pending: Map<string, OrchestrationV2ThreadLaunchInput>,
) {
  const original = pending.get(request.operation);
  const rpc = yield* openEnvironmentRpc(session);
  const command = original?.initialMessage
    ? {
        ...original,
        initialMessage: {
          ...original.initialMessage,
          attachments: yield* retryAttachments(
            session,
            rpc,
            original.initialMessage.attachments,
            request.files,
            request.operation,
            request.operation,
            () => pending.delete(request.operation),
          ),
        },
      }
    : (original ?? (yield* prepareLaunch(session, request, rpc)));
  pending.set(request.operation, command);
  const result = yield* rpc[ORCHESTRATION_V2_WS_METHODS.launchThread](command).pipe(
    Effect.tapError((error) =>
      Effect.gen(function* () {
        if (error._tag === "EnvironmentAuthorizationError" && !original) {
          pending.delete(request.operation);
        } else if (error._tag === "OrchestrationV2ThreadLaunchError") {
          // A launch error can follow a committed first message. Only absence proves the draft is editable.
          const api = yield* makeEnvironmentHttpApiGroupClient(
            session.httpBaseUrl,
            "orchestration",
          );
          yield* api
            .threadBoundedSnapshot({
              headers: headersFor(session),
              params: { threadId: ThreadId.make(request.operation) },
              query: { deferWork: "1", compactTurnItems: "1", dedupeBodies: "1" },
            })
            .pipe(
              Effect.tapError((lookup) =>
                Effect.sync(() => {
                  if (
                    lookup._tag === "EnvironmentResourceNotFoundError" &&
                    lookup.reason === "thread_not_found"
                  )
                    pending.delete(request.operation);
                }),
              ),
              Effect.ignore,
            );
        }
      }),
    ),
  );
  pending.delete(request.operation);
  const thread = result.projection.thread;
  return {
    sequence: result.sequence,
    thread: {
      id: result.threadId,
      recent_at: DateTime.toEpochMillis(thread.updatedAt),
      sort_at: DateTime.toEpochMillis(thread.createdAt),
      order_key: thread.activeOrderKey ?? null,
      project: thread.projectId,
      title: thread.title,
      branch: thread.branch,
      model: thread.modelSelection.model,
      status: result.projection.runs.at(-1)?.status ?? "idle",
      settled: thread.settledOverride === "settled",
    },
  };
});
