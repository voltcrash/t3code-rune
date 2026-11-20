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
  WS_METHODS,
  CommandId,
  ProjectId,
  isListedThreadLineage,
  ThreadId,
  type OrchestrationV2ProjectedTurnItem,
} from "@t3tools/contracts";
import { withOrchestrationProtocolHeader } from "@t3tools/client-runtime/state/environmentHttpAuth";
import { boundedSnapshotProjection } from "@t3tools/shared/orchestrationV2BoundedSnapshot";
import {
  findProjectByPath,
  inferProjectTitleFromPath,
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
    scopes: [AuthOrchestrationReadScope, AuthOrchestrationOperateScope, AuthFilesystemReadScope],
    clientMetadata: { label: "T3 Code Desktop", deviceType: "desktop" },
  });
  const scopes = parseOAuthScope(session.scope) ?? [];
  return {
    label: descriptor.label,
    platform: descriptor.platform.os,
    httpBaseUrl: target.httpBaseUrl,
    token: session.access_token,
    canBrowse: scopes.includes(AuthFilesystemReadScope),
    canEditProjects: scopes.includes(AuthOrchestrationOperateScope),
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

type Session = Awaited<ReturnType<typeof pairWithEnvironment>>;

const headersFor = (session: Session) =>
  withOrchestrationProtocolHeader({ authorization: `Bearer ${session.token}` });

const loadWorkspace = Effect.fn("nativeDesktop.workspace")(function* (session: Session) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const snapshot = yield* client.shellSnapshot({
    headers: headersFor(session),
    query: { omitIdleSubagents: "1" },
  });
  if ("resumable" in snapshot) return yield* Effect.die("Unexpected resume without a shell cursor");
  return {
    projects: snapshot.projects.map((project) => ({
      id: project.id,
      title: project.title,
      path: project.workspaceRoot,
      archived: project.archivedAt != null,
    })),
    threads: snapshot.threads
      .filter(
        (thread) =>
          thread.archivedAt === null &&
          thread.deletedAt === null &&
          isListedThreadLineage(thread.lineage),
      )
      .toSorted((a, b) => DateTime.toEpochMillis(b.updatedAt) - DateTime.toEpochMillis(a.updatedAt))
      .map((thread) => ({
        id: thread.id,
        project: thread.projectId,
        title: thread.title,
        branch: thread.branch,
        status: thread.status,
        settled: thread.settledOverride === "settled",
      })),
  };
});

export const readWorkspace = (session: Session) =>
  Effect.runPromise(
    loadWorkspace(session).pipe(Effect.timeout("20 seconds"), Effect.provide(session.http)),
    { signal: session.signal },
  );

function conversationRows(items: readonly OrchestrationV2ProjectedTurnItem[]) {
  return items.flatMap(({ item, sourceItemId, sourceThreadId }) => {
    if (item.type !== "user_message" && item.type !== "assistant_message") return [];
    return [
      {
        key: JSON.stringify([sourceThreadId, sourceItemId]),
        role: item.type === "user_message" ? "user" : "assistant",
        body: item.text,
        attachments: (item.attachments ?? []).map((attachment) => attachment.name),
      },
    ];
  });
}

const loadThread = Effect.fn("nativeDesktop.thread")(function* (
  session: Session,
  request: Readonly<{
    thread: string;
    cursor?: string;
    sequence?: number;
    oldest?: string;
    earlier_pages: number;
  }>,
) {
  const client = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const params = { threadId: ThreadId.make(request.thread) };
  const headers = headersFor(session);
  const pageBefore = (cursor: string) =>
    client.threadHistoryPage({
      headers,
      params,
      query: { cursor, view: "conversation", deferWork: "1" },
    });
  if (request.cursor) {
    const page = yield* pageBefore(request.cursor);
    return {
      type: "thread",
      thread: request.thread,
      sequence: page.snapshotSequence,
      older: true,
      earlier_pages: 1,
      rows: conversationRows(page.items),
      cursor: page.hasMoreHistory ? page.nextCursor : null,
    };
  }
  const bounded = yield* client.threadBoundedSnapshot({
    headers,
    params,
    query: { deferWork: "1", dedupeBodies: "1", compactTurnItems: "1" },
  });
  if (bounded.snapshotSequence === request.sequence) return { type: "unchanged" };

  const pages = [conversationRows(boundedSnapshotProjection(bounded).visibleTurnItems)];
  let cursor = bounded.hasMoreHistory ? bounded.historyCursor : null;
  while (
    cursor &&
    pages.length <= request.earlier_pages &&
    !pages[pages.length - 1]?.some((row) => row.key === request.oldest)
  ) {
    const page = yield* pageBefore(cursor);
    pages.push(conversationRows(page.items));
    cursor = page.hasMoreHistory ? page.nextCursor : null;
  }
  return {
    type: "thread",
    thread: request.thread,
    sequence: bounded.snapshotSequence,
    older: false,
    earlier_pages: pages.length - 1,
    rows: pages.reverse().flat(),
    cursor,
  };
});

export const readThread = (session: Session, request: Parameters<typeof loadThread>[1]) =>
  Effect.runPromise(
    loadThread(session, request).pipe(Effect.timeout("20 seconds"), Effect.provide(session.http)),
    { signal: session.signal },
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
