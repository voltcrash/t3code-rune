import { projectGlyph } from "./projectIcon.ts";
import { legacyThreadVisits, visitInput, type OpenVisitDatabase } from "./visits.ts";
import { recordServerClock, toServerClock } from "@t3tools/client-runtime/clock";
import * as Clock from "effect/Clock";
import {
  hasUnseenCompletion,
  resolveSidebarRowStatus,
  resolveThreadWindowStatus,
  shouldRecedeSidebarThread,
} from "@t3tools/client-runtime/state/sidebar-row-status";
import {
  getThreadSortTimestamp,
  resolveSettledThreadTimestamp,
  resolveThreadActivityTimestamp,
} from "@t3tools/client-runtime/state/thread-sort";
import {
  presentThreadShell,
  resolveThreadProviderStack,
  threadRuntimeCanArchive,
  resolveThreadWorkingStartedAt,
} from "@t3tools/client-runtime/state/models";
import { threadRaisedHandWhileSnoozed } from "@t3tools/client-runtime/state/thread-settled";
import { threadSearchSubject } from "@t3tools/client-runtime/state/thread-search";
import { normalizePlanMarkdownForExport } from "@t3tools/shared/proposedPlanText";
import { threadRecentUse } from "@t3tools/shared/paletteSearch";
import { bootstrapRemoteBearerSession } from "@t3tools/client-runtime/authorization";
import { fetchRemoteEnvironmentDescriptor } from "@t3tools/client-runtime/environment";
import {
  layerRemoteHttpClient,
  makeEnvironmentHttpApiGroupClient,
} from "@t3tools/client-runtime/rpc";
import {
  resolveEnvironmentMachineKind,
  AuthPreviewOperateScope,
  AuthDiagnosticsReadScope,
  AuthOrchestrationReadScope,
  AuthOrchestrationOperateScope,
  AuthFilesystemReadScope,
  AuthFilesystemWriteScope,
  AuthSourceControlWriteScope,
  AuthSettingsWriteScope,
  AuthTerminalReadScope,
  AuthTerminalOperateScope,
  WS_METHODS,
  CommandId,
  ProjectId,
  ThreadId,
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
import { resolveMarkdownFileLink } from "@t3tools/shared/markdownLinks";
import {
  isHostPath,
  normalizeProjectPathForDispatch,
  resolveWorkspaceFilePath,
  workspaceRelativeFilePath,
} from "@t3tools/shared/path";
import {
  WORKSPACE_BASENAME_LOOKUP_LIMIT,
  needsWorkspaceBasenameLookup,
  pickWorkspaceLinkMatch,
  workspaceLinkLookupQuery,
} from "@t3tools/client-runtime/workspace-basename-lookup";
import * as SecureRandom from "effect/crypto/SecureRandom";
import { THREAD_STATUS_TEXT } from "@t3tools/client-runtime/state/thread-activity-announcement";
import { m } from "../i18n/catalog.ts";
import { permissionLabel } from "./permissions.ts";
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
      ...(descriptor.capabilities.serverBrowser ? [AuthPreviewOperateScope] : []),
      AuthDiagnosticsReadScope,
      AuthOrchestrationReadScope,
      AuthOrchestrationOperateScope,
      AuthFilesystemReadScope,
      AuthFilesystemWriteScope,
      AuthSourceControlWriteScope,
      AuthSettingsWriteScope,
      AuthTerminalReadScope,
      AuthTerminalOperateScope,
    ],
    clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
  }).pipe(Effect.provideService(FetchHttpClient.RequestInit, { redirect: "error" }));
  const scopes = parseOAuthScope(session.scope) ?? [];
  return {
    environmentId: descriptor.environmentId,
    label: descriptor.label,
    machine: descriptor.platform.machine,
    serverVersion: descriptor.serverVersion,
    tracksClones: descriptor.capabilities.projectCloneTracking === true,
    vcsBranchStatus: descriptor.capabilities.vcsBranchStatus === true,
    browserAvailable:
      descriptor.capabilities.serverBrowser === true && scopes.includes(AuthOrchestrationReadScope),
    browserWritable: scopes.includes(AuthPreviewOperateScope),
    canReadIssues:
      descriptor.capabilities.issues === true && scopes.includes(AuthOrchestrationReadScope),
    canPin: descriptor.capabilities.threadPinning === true,
    visitTracking: descriptor.capabilities.threadVisitedTracking === true,
    shellThreadFamilies: descriptor.capabilities.shellThreadFamilies === true,
    streamsCommandOutput: descriptor.capabilities.commandOutputStreaming === true,
    attachmentUploads: descriptor.capabilities.attachmentUploads === true,
    voiceTranscription: descriptor.capabilities.voiceTranscription === true,
    maxFileBytes: descriptor.capabilities.fileAttachments?.maxUploadBytes ?? 0,
    platform: descriptor.platform.os,
    httpBaseUrl: target.httpBaseUrl,
    token: session.access_token,
    canReadDiagnostics: scopes.includes(AuthDiagnosticsReadScope),
    canWriteGit: scopes.includes(AuthSourceControlWriteScope),
    canReadTerminal: scopes.includes(AuthTerminalReadScope),
    canOperateTerminal: scopes.includes(AuthTerminalOperateScope),
    canWriteFiles: scopes.includes(AuthFilesystemWriteScope),
    canBrowse: scopes.includes(AuthFilesystemReadScope),
    canOperate: scopes.includes(AuthOrchestrationOperateScope),
    canWriteSettings: scopes.includes(AuthSettingsWriteScope),
  };
});

export async function pairWithTarget(
  target: Pick<ResolvedRemotePairingTarget, "credential" | "httpBaseUrl">,
  signal: AbortSignal,
  http: typeof remoteHttp = localHttp,
  openVisits?: OpenVisitDatabase,
) {
  const session = await Effect.runPromise(pair(target).pipe(Effect.provide(http)), { signal });
  return {
    ...session,
    http,
    signal,
    legacyVisits: openVisits ? legacyThreadVisits(openVisits, session.environmentId) : undefined,
  };
}

export function pairWithEnvironment(
  link: string,
  signal: AbortSignal,
  openVisits?: OpenVisitDatabase,
) {
  const target = resolveRemotePairingTarget({ pairingUrl: readPairingAppLink(link) ?? link });
  return pairWithTarget(target, signal, remoteHttp, openVisits);
}

export type Session = Awaited<ReturnType<typeof pairWithEnvironment>>;

export const headersFor = (session: Session) =>
  withOrchestrationProtocolHeader({ authorization: `Bearer ${session.token}` });

export function projectSummary(
  project: Pick<
    Project,
    | "id"
    | "title"
    | "workspaceRoot"
    | "archivedAt"
    | "updatedAt"
    | "projectIcon"
    | "faviconPath"
    | "defaultModelSelection"
  >,
  scratchRoot: string | undefined,
) {
  return {
    id: project.id,
    title: project.title,
    icon: projectGlyph(project.title, project.projectIcon),
    favicon: project.projectIcon ? null : { path: project.faviconPath ?? null },
    path: project.workspaceRoot,
    ...(project.defaultModelSelection && { defaultModelSelection: project.defaultModelSelection }),
    archived: project.archivedAt != null,
    activity_at: Date.parse(project.updatedAt),
    scratch: isScratchProject(project, scratchRoot),
  };
}

export function visibleThread(thread: OrchestrationV2ThreadShell) {
  return (
    thread.archivedAt === null && thread.deletedAt === null && isListedThreadLineage(thread.lineage)
  );
}

export function threadSummary(thread: OrchestrationV2ThreadShell, session: Session) {
  const shell = presentThreadShell(session.environmentId, thread);
  const localVisit =
    thread.lastVisitedAt === undefined ? session.legacyVisits?.get(thread.id) : undefined;
  const visited = localVisit ? { ...shell, lastVisitedAt: localVisit } : shell;
  const row = resolveSidebarRowStatus(visited);
  const titleStatus = resolveThreadWindowStatus(shell);
  const isUnread = hasUnseenCompletion(visited);
  const status =
    row === "working" && shell.goal?.status === "active"
      ? "goal"
      : row === "ready" && isUnread
        ? "done"
        : row;
  const timestamp =
    row === "ready"
      ? resolveThreadActivityTimestamp(shell)
      : row === "working" || row === "preparing"
        ? resolveThreadWorkingStartedAt(shell)
        : row === "waiting"
          ? shell.latestRun?.completedAt
          : null;
  return {
    id: thread.id,
    project: thread.projectId,
    title: thread.title,
    branch: thread.branch,
    ...(thread.worktreePath ? { worktree: thread.worktreePath } : {}),
    status,
    can_archive: threadRuntimeCanArchive(shell.runtime),
    ...(titleStatus && { title_status: m(THREAD_STATUS_TEXT[titleStatus]) }),
    status_at: timestamp ? Date.parse(timestamp) : null,
    recedes: shouldRecedeSidebarThread({
      status: row,
      isUnread,
      isWoke: false,
      isActive: false,
      isSelected: false,
    }),
    providers: resolveThreadProviderStack(shell),
    model: thread.modelSelection.model,
    permission: {
      instance: thread.modelSelection.instanceId,
      mode: thread.runtimeMode,
      label: permissionLabel(thread.runtimeMode),
    },
    snoozed_until:
      shell.snoozedUntil && !threadRaisedHandWhileSnoozed(shell)
        ? Date.parse(shell.snoozedUntil)
        : null,
    settled: thread.settledOverride === "settled",
    activity_at: getThreadSortTimestamp(shell, "updated_at"),
    recent_at: threadRecentUse(threadSearchSubject(shell, null)),
    pinned: thread.pinnedAt != null,
    order_key: (thread.pinnedAt == null ? thread.activeOrderKey : thread.pinOrderKey) ?? null,
    sort_at:
      thread.settledOverride === "settled"
        ? Date.parse(resolveSettledThreadTimestamp(shell) ?? "") || 0
        : thread.pinnedAt != null
          ? DateTime.toEpochMillis(thread.createdAt)
          : Math.max(
              DateTime.toEpochMillis(thread.createdAt),
              DateTime.toEpochMillis(thread.unsettledAt ?? thread.createdAt),
            ),
  };
}

export function workspaceRows(
  snapshot: OrchestrationV2ShellSnapshot,
  session: Session,
  scratchRoot: string | undefined,
) {
  const listed = snapshot.threads.filter(visibleThread);
  return {
    legacyScripts: new Map(
      snapshot.projects
        .filter((project) => project.scripts.length)
        .map((project) => [String(project.id), project.scripts]),
    ),
    visitRows: new Map<string, ReturnType<typeof visitInput>>(
      listed.map((t) => [t.id, visitInput(session.environmentId, t)]),
    ),
    view: {
      scratch_available: Boolean(scratchRoot),
      projects: snapshot.projects.map((project) => projectSummary(project, scratchRoot)),
      threads: listed.map((row) => threadSummary(row, session)),
    },
  };
}

export const sampleDesktopConfig = Effect.fn("nativeDesktop.sampleConfig")(function* (
  rpc: Effect.Success<ReturnType<typeof openEnvironmentRpc>>,
  session: Session,
) {
  const requestedAtMs = yield* Clock.currentTimeMillis;
  const config = yield* rpc[WS_METHODS.serverGetConfig]({});
  const workspace = {
    scratchRoot: config.scratchWorkspaceRoot,
    machine: resolveEnvironmentMachineKind(config),
  };
  const serverTimeMs = Date.parse(config.serverTime ?? "");
  if (!Number.isFinite(serverTimeMs)) return { ...workspace, clockOffset: undefined };
  recordServerClock(session.environmentId, {
    requestedAtMs,
    receivedAtMs: yield* Clock.currentTimeMillis,
    serverTimeMs,
  });
  return {
    ...workspace,
    clockOffset: toServerClock(session.environmentId, 0),
  };
});

export const loadWorkspace = Effect.fn("nativeDesktop.workspace")(function* (session: Session) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const [snapshot, config] = yield* Effect.all(
    [
      client.shellSnapshot({
        headers: headersFor(session),
        query: session.shellThreadFamilies ? { omitIdleSubagents: "1" } : {},
      }),
      session.canOperate
        ? openEnvironmentRpc(session).pipe(
            Effect.flatMap((rpc) => sampleDesktopConfig(rpc, session)),
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
    configLoaded: config !== undefined,
    scratchRoot: config?.scratchRoot,
    machine: config?.machine ?? session.machine,
    clockOffset: config?.clockOffset,
    ...workspaceRows(snapshot, session, config?.scratchRoot),
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

export const readProjectRoot = Effect.fn("nativeDesktop.readProjectRoot")(function* (
  session: Session,
  projectId: string,
) {
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "projects");
  const { projects } = yield* api.snapshot({
    headers: { authorization: `Bearer ${session.token}` },
  });
  return projects.find((p) => p.id === projectId && p.deletedAt === null)?.workspaceRoot;
});

export const readThreadSettingsSnapshot = Effect.fn("nativeDesktop.threadSettingsSnapshot")(
  function* (session: Session, thread: string) {
    const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
    return yield* api.threadBoundedSnapshot({
      headers: headersFor(session),
      params: { threadId: ThreadId.make(thread) },
      query: { dedupeBodies: "1", compactTurnItems: "1", deferWork: "1" },
    });
  },
);

class ThreadWorkspaceChanged extends Schema.TaggedError<ThreadWorkspaceChanged>()(
  "ThreadWorkspaceChanged",
  {},
) {}

const threadWorkspaceRoot = Effect.fn("nativeDesktop.threadWorkspaceRoot")(function* (
  session: Session,
  threadId: string,
  expectedRoot: string,
) {
  const {
    projection: { thread },
  } = yield* readThreadSettingsSnapshot(session, threadId);
  const root = thread.worktreePath ?? (yield* readProjectRoot(session, thread.projectId));
  if (!root || root !== expectedRoot || thread.archivedAt || thread.deletedAt)
    return yield* new ThreadWorkspaceChanged();
  return root;
});

type WorkspaceFileInput =
  | { readonly action: "search" | "filter"; readonly query: string }
  | { readonly action: "read"; readonly path: string }
  | { readonly action: "list"; readonly directory: string };

export function workspaceMarkdownLink(href: string, root: string) {
  const position = resolveMarkdownFileLink(href, root)?.position;
  const path = position && workspaceRelativeFilePath(position.path, root);
  return path && path !== "." ? { path, line: position.line } : null;
}

/** Like the web client, a link that names no file as written opens the one file search finds. */
export const resolveWorkspaceMarkdownLink = Effect.fn("nativeDesktop.resolveWorkspaceMarkdownLink")(
  function* (
    session: Session,
    href: string,
    root: string,
    activeRpc?: Effect.Success<ReturnType<typeof openEnvironmentRpc>>,
  ) {
    const link = workspaceMarkdownLink(href, root);
    const query = link && workspaceLinkLookupQuery(link.path, root);
    if (!link || query == null) return link;
    const rpc = activeRpc ?? (yield* openEnvironmentRpc(session));
    if (!needsWorkspaceBasenameLookup(link.path)) {
      const { entries } = yield* rpc[WS_METHODS.filesystemGetMetadata]({
        paths: [resolveWorkspaceFilePath(link.path, root)],
      });
      if (entries[0] !== null) return link;
    }
    const { entries } = yield* rpc[WS_METHODS.projectsSearchEntries]({
      cwd: root,
      query,
      limit: WORKSPACE_BASENAME_LOOKUP_LIMIT,
      kind: "file",
    });
    return { ...link, path: pickWorkspaceLinkMatch(link.path, root, entries) ?? link.path };
  },
);

export const readWorkspaceFiles = Effect.fn("nativeDesktop.readWorkspaceFiles")(function* (
  session: Session,
  threadId: string,
  expectedRoot: string,
  input: WorkspaceFileInput,
  activeRpc: Effect.Success<ReturnType<typeof openEnvironmentRpc>> | undefined,
) {
  const cwd =
    input.action !== "read" && activeRpc
      ? expectedRoot
      : yield* threadWorkspaceRoot(session, threadId, expectedRoot);
  const rpc = activeRpc ?? (yield* openEnvironmentRpc(session));
  const list = () => {
    switch (input.action) {
      case "filter":
        return rpc[WS_METHODS.projectsSearchEntries]({
          cwd,
          query: input.query
            .trim()
            .slice(0, 256)
            .replace(/[\uD800-\uDBFF]$/, ""),
          limit: 200,
        }).pipe(
          Effect.map(({ entries, truncated }) => ({ type: "filteredfiles", entries, truncated })),
        );
      case "search":
        return rpc[WS_METHODS.projectsSearchEntries]({
          cwd,
          query: input.query,
          kind: "file",
          limit: 200,
        }).pipe(
          Effect.map(({ entries }) => ({
            type: "filematches",
            paths: entries.filter((e) => e.kind === "file").map((e) => e.path),
          })),
        );
      case "list":
        return rpc[WS_METHODS.projectsListEntries]({ cwd, directoryPath: input.directory }).pipe(
          Effect.map(({ entries, truncated }) => ({ type: "fileentries", entries, truncated })),
        );
      case "read":
        return rpc[WS_METHODS.projectsReadFile]({ cwd, relativePath: input.path }).pipe(
          Effect.map(({ contents, truncated }) => ({ type: "filesource", contents, truncated })),
        );
    }
  };
  const result = yield* list();
  if (!activeRpc) yield* threadWorkspaceRoot(session, threadId, expectedRoot);
  return result;
});

export const writePlanToWorkspace = Effect.fn("nativeDesktop.writePlanToWorkspace")(function* (
  session: Session,
  threadId: string,
  expectedRoot: string,
  relativePath: string,
  markdown: string,
) {
  const root = yield* threadWorkspaceRoot(session, threadId, expectedRoot);
  const rpc = yield* openEnvironmentRpc(session);
  return yield* rpc[WS_METHODS.projectsWriteFile]({
    cwd: root,
    relativePath,
    contents: normalizePlanMarkdownForExport(markdown),
  });
});
