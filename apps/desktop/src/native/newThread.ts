import { deriveThreadTitleSeed } from "@t3tools/client-runtime/operations";
import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  CommandId,
  DEFAULT_PROVIDER_INTERACTION_MODE,
  MessageId,
  ORCHESTRATION_V2_WS_METHODS,
  ProjectId,
  ThreadId,
  T3_PROJECT_FILE_NAME,
  WS_METHODS,
  type OrchestrationV2ThreadLaunchInput,
  type ServerConfig,
} from "@t3tools/contracts";
import { parseT3ProjectFile } from "@t3tools/shared/t3ProjectFile";
import { resolveNewThreadEnvMode, resolveProjectSettings } from "@t3tools/shared/projectSettings";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as DateTime from "effect/DateTime";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import * as Option from "effect/Option";
import { prepareAttachments, retryAttachments } from "./attachments.ts";
import { draftModelCatalog, draftModelReply } from "./draftModels.ts";
import { permissionLabel } from "./permissions.ts";
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
  currentConfig?: ServerConfig,
) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const [config, shell] = yield* Effect.all(
    [
      currentConfig ? Effect.succeed(currentConfig) : rpc[WS_METHODS.serverGetConfig]({}),
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
  const models = draftModelCatalog(
    config,
    projectId,
    record.workspaceRoot,
    settings,
    yield* Clock.currentTimeMillis,
  );
  return {
    record: projectSummary(record, config.scratchWorkspaceRoot),
    cwd: record.workspaceRoot,
    startFromOrigin: settings.newWorktreesStartFromOrigin,
    project,
    ...models,
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
  currentConfig?: ServerConfig,
) {
  const rpc = yield* openEnvironmentRpc(session);
  const project = existingProject ?? (yield* rpc[WS_METHODS.projectsEnsureScratch]({})).projectId;
  const options = yield* launchContext(session, project, rpc, currentConfig);
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
    ...draftModelReply(options),
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
    runtime_override?: string | null;
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
  const runtime = options.permissions[choice.selection.instanceId]?.find(
    ({ mode }) => mode === (request.runtime_override ?? request.runtime),
  );
  if (
    !runtime ||
    runtime.denied ||
    (request.runtime_override == null && choice.runtime !== request.runtime)
  )
    return yield* new NewThreadUnavailable({
      message:
        runtime?.denied ??
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
    runtimeMode: runtime.mode,
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
  const run = result.projection.runs.at(-1);
  const active =
    run && ["preparing", "queued", "starting", "running", "waiting"].includes(run.status);
  return {
    sequence: result.sequence,
    thread: {
      id: result.threadId,
      recent_at: DateTime.toEpochMillis(thread.updatedAt),
      sort_at: DateTime.toEpochMillis(thread.createdAt),
      pinned: thread.pinnedAt != null,
      order_key: (thread.pinnedAt == null ? thread.activeOrderKey : thread.pinOrderKey) ?? null,
      project: thread.projectId,
      title: thread.title,
      branch: thread.branch,
      model: thread.modelSelection.model,
      permission: {
        instance: thread.modelSelection.instanceId,
        mode: thread.runtimeMode,
        label: permissionLabel(thread.runtimeMode),
      },
      status:
        run?.status === "preparing"
          ? "preparing"
          : run?.status === "failed"
            ? "failed"
            : active
              ? "working"
              : "ready",
      status_at: active
        ? DateTime.toEpochMillis(run.startedAt ?? run.requestedAt)
        : DateTime.toEpochMillis(run?.completedAt ?? thread.updatedAt),
      recedes: run?.status !== "failed",
      settled: thread.settledOverride === "settled",
    },
  };
});
