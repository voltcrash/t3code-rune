import { bootstrapRemoteBearerSession } from "@t3tools/client-runtime/authorization";
import { fetchRemoteEnvironmentDescriptor } from "@t3tools/client-runtime/environment";
import {
  layerRemoteHttpClient,
  makeEnvironmentHttpApiGroupClient,
} from "@t3tools/client-runtime/rpc";
import {
  AuthOrchestrationReadScope,
  AuthOrchestrationOperateScope,
  AuthFilesystemReadScope,
  AuthTerminalReadScope,
  AuthTerminalOperateScope,
  WS_METHODS,
  CommandId,
  ProjectId,
  type Project,
  type OrchestrationV2ThreadShell,
  type OrchestrationV2ShellSnapshot,
  isListedThreadLineage,
} from "@t3tools/contracts";
import { withOrchestrationProtocolHeader } from "@t3tools/client-runtime/state/environmentHttpAuth";
import {
  findProjectByPath,
  inferProjectTitleFromPath,
  isScratchProject,
  isUnsupportedWindowsProjectPath,
} from "@t3tools/client-runtime/state/projects";
import { parseOAuthScope } from "@t3tools/shared/oauthScope";
import { isHostPath, normalizeProjectPathForDispatch } from "@t3tools/shared/path";
import * as SecureRandom from "effect/crypto/SecureRandom";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import * as NodePath from "@effect/platform-node/NodePath";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as DateTime from "effect/DateTime";
import {
  readPairingAppLink,
  resolveRemotePairingTarget,
  type ResolvedRemotePairingTarget,
} from "@t3tools/shared/remote";
import * as Effect from "effect/Effect";
import { FetchHttpClient } from "effect/http";
import * as Layer from "effect/Layer";

const remoteHttp = layerRemoteHttpClient(globalThis.fetch);
const localHttp = Layer.merge(
  remoteHttp,
  Layer.succeed(FetchHttpClient.RequestInit, { redirect: "error" }),
);

const pair = Effect.fn("nativeDesktop.pair")(function* (
  target: Pick<ResolvedRemotePairingTarget, "credential" | "httpBaseUrl">,
) {
  const descriptor = yield* fetchRemoteEnvironmentDescriptor(target);
  const session = yield* bootstrapRemoteBearerSession({
    ...target,
    scopes: [
      AuthOrchestrationReadScope,
      AuthOrchestrationOperateScope,
      AuthFilesystemReadScope,
      AuthTerminalReadScope,
      AuthTerminalOperateScope,
    ],
    clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
  });
  const scopes = parseOAuthScope(session.scope) ?? [];
  return {
    label: descriptor.label,
    serverVersion: descriptor.serverVersion,
    shellThreadFamilies: descriptor.capabilities.shellThreadFamilies === true,
    streamsCommandOutput: descriptor.capabilities.commandOutputStreaming === true,
    attachmentUploads: descriptor.capabilities.attachmentUploads === true,
    maxFileBytes: descriptor.capabilities.fileAttachments?.maxUploadBytes ?? 0,
    platform: descriptor.platform.os,
    httpBaseUrl: target.httpBaseUrl,
    token: session.access_token,
    canReadTerminal: scopes.includes(AuthTerminalReadScope),
    canOperateTerminal: scopes.includes(AuthTerminalOperateScope),
    canBrowse: scopes.includes(AuthFilesystemReadScope),
    canOperate: scopes.includes(AuthOrchestrationOperateScope),
  };
});

export async function pairWithTarget(
  target: Pick<ResolvedRemotePairingTarget, "credential" | "httpBaseUrl">,
  signal: AbortSignal,
  http: typeof remoteHttp = localHttp,
) {
  const session = await Effect.runPromise(pair(target).pipe(Effect.provide(http)), { signal });
  return { ...session, http, signal };
}

export function pairWithEnvironment(link: string, signal: AbortSignal) {
  const target = resolveRemotePairingTarget({ pairingUrl: readPairingAppLink(link) ?? link });
  return pairWithTarget(target, signal, remoteHttp);
}

export type Session = Awaited<ReturnType<typeof pairWithEnvironment>>;

export const headersFor = (session: Session) =>
  withOrchestrationProtocolHeader({ authorization: `Bearer ${session.token}` });

export function projectSummary(
  project: Pick<Project, "id" | "title" | "workspaceRoot" | "archivedAt">,
  scratchRoot: string | undefined,
) {
  return {
    id: project.id,
    title: project.title,
    path: project.workspaceRoot,
    archived: project.archivedAt != null,
    scratch: isScratchProject(project, scratchRoot),
  };
}

export function visibleThread(thread: OrchestrationV2ThreadShell) {
  return (
    thread.archivedAt === null && thread.deletedAt === null && isListedThreadLineage(thread.lineage)
  );
}

export function threadSummary(thread: OrchestrationV2ThreadShell) {
  return {
    id: thread.id,
    project: thread.projectId,
    title: thread.title,
    branch: thread.branch,
    status: thread.status,
    model: thread.modelSelection.model,
    settled: thread.settledOverride === "settled",
    order_key: thread.activeOrderKey ?? null,
    sort_at:
      thread.settledOverride === "settled"
        ? DateTime.toEpochMillis(
            thread.settledAt ?? thread.latestRunCompletedAt ?? thread.updatedAt,
          )
        : Math.max(
            DateTime.toEpochMillis(thread.createdAt),
            DateTime.toEpochMillis(thread.unsettledAt ?? thread.createdAt),
          ),
  };
}

export function workspaceRows(
  snapshot: OrchestrationV2ShellSnapshot,
  scratchRoot: string | undefined,
) {
  return {
    scratch_available: Boolean(scratchRoot),
    projects: snapshot.projects.map((project) => projectSummary(project, scratchRoot)),
    threads: snapshot.threads.filter(visibleThread).map(threadSummary),
  };
}

const readScratchRoot = Effect.fn("nativeDesktop.readScratchRoot")(function* (session: Session) {
  const rpc = yield* openEnvironmentRpc(session);
  return (yield* rpc[WS_METHODS.serverGetConfig]({})).scratchWorkspaceRoot;
});

export const loadWorkspace = Effect.fn("nativeDesktop.workspace")(function* (session: Session) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const [snapshot, scratchRoot] = yield* Effect.all(
    [
      client.shellSnapshot({
        headers: headersFor(session),
        query: session.shellThreadFamilies ? { omitIdleSubagents: "1" } : {},
      }),
      session.canOperate
        ? readScratchRoot(session).pipe(
            Effect.scoped,
            Effect.timeout("3 seconds"),
            Effect.orElseSucceed(() => undefined),
          )
        : Effect.undefined,
    ],
    { concurrency: "unbounded" },
  );
  if ("resumable" in snapshot) return yield* Effect.die("Unexpected resume without a shell cursor");
  return {
    sequence: snapshot.snapshotSequence,
    identities: snapshot.projectIdentities,
    scratchRoot,
    view: workspaceRows(snapshot, scratchRoot),
  };
});

export const readWorkspace = (session: Session, signal = session.signal) =>
  Effect.runPromise(
    loadWorkspace(session).pipe(Effect.timeout("20 seconds"), Effect.provide(session.http)),
    { signal },
  );

type ProjectEdit =
  | { type: "addproject"; path: string }
  | { type: "archiveproject"; project: string; archived: boolean };

class InvalidProjectPath extends Schema.TaggedError<InvalidProjectPath>()(
  "InvalidProjectPath",
  {},
) {}

export class HomePathNeedsBrowseAccess extends Schema.TaggedError<HomePathNeedsBrowseAccess>()(
  "HomePathNeedsBrowseAccess",
  {},
) {}

export const resolveFolder = Effect.fn("nativeDesktop.resolveFolder")(function* (
  session: Pick<Session, "httpBaseUrl" | "token" | "platform" | "canBrowse">,
  input: string,
) {
  const value = normalizeProjectPathForDispatch(input);
  if (
    (value !== "~" && !isHostPath(value)) ||
    isUnsupportedWindowsProjectPath(value, session.platform)
  )
    return yield* new InvalidProjectPath();
  const path = yield* Path.Path;
  if (value === "~" || value.startsWith("~/") || value.startsWith("~\\")) {
    if (!session.canBrowse) return yield* new HomePathNeedsBrowseAccess();
    const rpc = yield* openEnvironmentRpc(session);
    const folder = yield* rpc[WS_METHODS.filesystemBrowse]({ partialPath: value + path.sep });
    return folder.parentPath;
  }
  const normalized = path.normalize(value);
  if (
    !path.isAbsolute(normalized) ||
    (session.platform === "windows" && path.parse(normalized).root.length < 2)
  ) {
    return yield* new InvalidProjectPath();
  }
  return normalizeProjectPathForDispatch(normalized);
});

const editProject = Effect.fn("nativeDesktop.editProject")(function* (
  session: Session,
  edit: ProjectEdit,
) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "projects");
  const headers = { authorization: `Bearer ${session.token}` };
  const random = yield* SecureRandom.SecureRandom;
  const commandId = CommandId.make(yield* random.uuidV4());
  if (edit.type === "archiveproject") {
    return yield* client.mutate({
      headers,
      payload: {
        type: "project.update",
        commandId,
        projectId: ProjectId.make(edit.project),
        archived: edit.archived,
      },
    });
  }
  const workspaceRoot = yield* resolveFolder(session, edit.path);
  const snapshot = yield* client.snapshot({ headers });
  const existing = findProjectByPath(
    snapshot.projects.filter((p) => p.deletedAt === null),
    workspaceRoot,
  );
  if (existing) {
    if (existing.archivedAt === null) return existing;
    return yield* client.mutate({
      headers,
      payload: {
        type: "project.update",
        commandId,
        projectId: existing.id,
        archived: false,
      },
    });
  }
  return yield* client.mutate({
    headers,
    payload: {
      type: "project.create",
      commandId,
      projectId: ProjectId.make(yield* random.uuidV4()),
      title: inferProjectTitleFromPath(workspaceRoot),
      workspaceRoot,
    },
  });
});

export function changeProject(session: Session, edit: ProjectEdit) {
  return Effect.runPromise(
    editProject(session, edit).pipe(
      Effect.scoped,
      Effect.timeout("20 seconds"),
      Effect.provide([
        session.http,
        SecureRandom.layerWebCrypto,
        session.platform === "windows" ? NodePath.layerWin32 : NodePath.layerPosix,
      ]),
    ),
    { signal: session.signal },
  );
}
