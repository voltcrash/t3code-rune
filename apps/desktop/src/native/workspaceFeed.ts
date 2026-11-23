import { serializeComposerFileLink } from "@t3tools/shared/composerTrigger";
import { watchProjectFileActions } from "./projectFile.ts";
import { topOfPinnedRunOrderKey } from "@t3tools/shared/pinOrderKey";
import { sortProjectsByLatestActivity } from "@t3tools/client-runtime/state/thread-sort";
import { isScratchProject } from "@t3tools/client-runtime/state/projects";
import { watchProjectScripts } from "./projectScripts.ts";
import { watchWorkspaceGit } from "./workspaceGit.ts";
import { watchDraftModels } from "./draftModels.ts";
import { visitInput, type trackVisibleThread } from "./visits.ts";
import { ORCHESTRATION_V2_WS_METHODS, WS_METHODS } from "@t3tools/contracts";
import * as NodeUtil from "node:util";
import * as Effect from "effect/Effect";
import * as Schedule from "effect/Schedule";
import * as Stream from "effect/Stream";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import {
  sampleDesktopConfig,
  projectSummary,
  loadWorkspace,
  readWorkspace,
  threadSummary,
  visibleThread,
  workspaceRows,
  type Session,
} from "./projects.ts";

type Loaded = Awaited<ReturnType<typeof readWorkspace>>;
type ProjectRow = ReturnType<typeof projectSummary>;
type ThreadRow = ReturnType<typeof threadSummary>;
type Changes = {
  landing_project: string | undefined;
  sequence: number;
  scratch_available: boolean;
  threads: ThreadRow[];
  projects: ProjectRow[];
  removed_threads: string[];
  removed_projects: string[];
};

function rowsById<T extends { id: string }>(rows: readonly T[]) {
  return new Map<string, T>(rows.map((row) => [row.id, row]));
}

function changeRow<T>(rows: Map<string, T>, dirty: Set<string>, id: string, row: T | undefined) {
  if (row === undefined) {
    if (!rows.delete(id)) return;
  } else {
    if (NodeUtil.isDeepStrictEqual(rows.get(id), row)) return;
    rows.set(id, row);
  }
  dirty.add(id);
}

function reconcileRows<T extends { id: string }>(
  rows: Map<string, T>,
  dirty: Set<string>,
  next: T[],
) {
  const incoming = rowsById(next);
  for (const id of rows.keys()) {
    if (!incoming.has(id)) changeRow(rows, dirty, id, undefined);
  }
  for (const row of next) changeRow(rows, dirty, row.id, row);
}

function takeChanges<T>(rows: Map<string, T>, dirty: Set<string>) {
  const changed = [];
  const removed = [];
  for (const id of dirty) {
    const row = rows.get(id);
    if (row === undefined) removed.push(id);
    else changed.push(row);
  }
  dirty.clear();
  return { changed, removed };
}

export function watchWorkspace(
  session: Session,
  initial: Loaded,
  signal: AbortSignal,
  emit: (changes: Changes) => void,
  paused: () => void,
  onClock: (offset: number) => void,
  refreshFavicons: (projects?: string[]) => void,
  currentVisit: () => ReturnType<typeof trackVisibleThread> | undefined,
  onWorkspaceConfig: Parameters<typeof watchDraftModels>[3],
  onSynchronized: (ready: boolean) => void,
) {
  let { sequence, identities, scratchRoot, visitRows } = initial;
  const projects = rowsById(initial.view.projects);
  const threads = rowsById(initial.view.threads);
  let configLoaded = initial.configLoaded;
  let legacyScripts = initial.legacyScripts;
  const scripts = watchProjectScripts(
    (id) => {
      const project = projects.get(id);
      return project && !project.archived && !project.scratch
        ? (legacyScripts.get(id) ?? [])
        : undefined;
    },
    () => legacyScripts,
  );
  let activeRpc: Effect.Success<ReturnType<typeof openEnvironmentRpc>> | undefined;
  let scriptTarget: { project: string; thread: string | null } | undefined;
  const fileActions = watchProjectFileActions(
    signal,
    () => {
      if (
        !activeRpc ||
        !scriptTarget ||
        !session.canBrowse ||
        !session.canOperate ||
        !session.canWriteSettings
      )
        return;
      const project = projects.get(scriptTarget.project);
      if (!project || project.archived || project.scratch) return;
      const thread = scriptTarget.thread === null ? undefined : threads.get(scriptTarget.thread);
      if (scriptTarget.thread !== null && thread?.project !== project.id) return;
      return { rpc: activeRpc, cwd: thread?.worktree ?? project.path };
    },
    scripts.setFileActions,
  );
  const git = watchWorkspaceGit(signal, session, (target) => {
    const thread = target.draft ? undefined : threads.get(target.target);
    const project = projects.get(target.draft ? target.target : (thread?.project ?? ""));
    return (
      configLoaded &&
      project !== undefined &&
      !project.scratch &&
      !isScratchProject({ workspaceRoot: project.path }, scratchRoot) &&
      (thread?.worktree ?? project.path) === target.cwd
    );
  });
  const draftModels = watchDraftModels(
    signal,
    (id) => projects.get(id),
    session.environmentId,
    (config, rpc) => {
      if (config && (!configLoaded || config.scratchWorkspaceRoot !== scratchRoot)) {
        configLoaded = true;
        scratchRoot = config.scratchWorkspaceRoot;
        git.reconcile();
      }
      scripts.configure(config?.settings);
      onWorkspaceConfig(config, rpc);
    },
  );
  const dirtyProjects = new Set<string>();
  const dirtyThreads = new Set<string>();
  function pickLandingProject() {
    if (!session.canOperate) return undefined;
    const active = [...projects.values()].filter((p) => !p.archived);
    if (active.length <= 1) return active[0]?.id;
    const latest = new Map<string, number>();
    for (const row of threads.values()) {
      latest.set(row.project, Math.max(latest.get(row.project) ?? -Infinity, row.activity_at));
    }
    return sortProjectsByLatestActivity(active, (p) => latest.get(p.id) ?? p.activity_at)[0]?.id;
  }
  const initialLanding = pickLandingProject();
  let landing = initialLanding;
  let scratchAvailable = initial.view.scratch_available;
  let scratchChanged = false;
  let waiting = false;
  let scheduled = false;
  let lastPin: { key: string; sequence: number } | undefined;
  let publishedSequence = sequence;
  let requestedSequence = sequence;
  const flush = () => {
    if (
      scheduled ||
      waiting ||
      signal.aborted ||
      (!scratchChanged &&
        !dirtyThreads.size &&
        !dirtyProjects.size &&
        !(publishedSequence < requestedSequence && sequence >= requestedSequence))
    )
      return;
    scheduled = true;
    void Effect.runPromise(
      Effect.sleep("16 millis").pipe(
        Effect.andThen(
          Effect.sync(() => {
            scheduled = false;
            git.reconcile();
            draftModels.projectChanged(dirtyProjects);
            const p = takeChanges(projects, dirtyProjects);
            const t = takeChanges(threads, dirtyThreads);
            waiting = true;
            publishedSequence = sequence;
            if (
              (p.changed.length || p.removed.length) &&
              (!landing || projects.get(landing)?.archived !== false)
            ) {
              landing = pickLandingProject();
            }
            emit({
              landing_project: landing,
              sequence,
              scratch_available: scratchAvailable,
              projects: p.changed,
              threads: t.changed,
              removed_projects: p.removed,
              removed_threads: t.removed,
            });
            scratchChanged = false;
          }),
        ),
      ),
      { signal },
    ).catch(() => {});
  };
  function replace(fresh: Pick<Loaded, "view" | "visitRows" | "legacyScripts">) {
    const { view } = fresh;
    visitRows = fresh.visitRows;
    legacyScripts = fresh.legacyScripts;
    reconcileRows(projects, dirtyProjects, view.projects);
    reconcileRows(threads, dirtyThreads, view.threads);
    fileActions.reconcile();
    scripts.publish();
    scratchChanged ||= view.scratch_available !== scratchAvailable;
    scratchAvailable = view.scratch_available;
    const active = currentVisit();
    if (active) active.update(visitRows.get(active.id) ?? null);
    flush();
  }
  function adopt(fresh: Loaded) {
    sequence = fresh.sequence;
    identities = fresh.identities;
    configLoaded = fresh.configLoaded;
    scratchRoot = fresh.scratchRoot;
    if (fresh.clockOffset !== undefined) onClock(fresh.clockOffset);
    replace(fresh);
  }
  let sampleOnConnect = initial.clockOffset === undefined;
  let failures = 0;
  let needSnapshot = false;
  let retryFavicons = false;
  let completedClones = new Map<string, string>();
  const subscribe = Effect.fn("nativeDesktop.followWorkspace")(function* () {
    if (needSnapshot) {
      const fresh = yield* loadWorkspace(session).pipe(Effect.timeout("20 seconds"));
      adopt(fresh);
      needSnapshot = false;
    }
    const rpc = yield* openEnvironmentRpc(session);
    draftModels.connect(rpc);
    yield* Effect.addFinalizer(() =>
      Effect.sync(() => {
        git.connect(undefined);
        draftModels.connect(undefined);
        if (activeRpc && !signal.aborted) onSynchronized(false);
        activeRpc = undefined;
        fileActions.reconcile();
        retryFavicons = true;
      }),
    );
    if (sampleOnConnect) {
      yield* sampleDesktopConfig(rpc, session).pipe(
        Effect.timeout("3 seconds"),
        Effect.tap(({ clockOffset, scratchRoot: root }) =>
          Effect.sync(() => {
            configLoaded = true;
            scratchRoot = root;
            git.reconcile();
            if (clockOffset !== undefined) onClock(clockOffset);
          }),
        ),
        Effect.ignore,
        Effect.forkScoped,
      );
    }
    sampleOnConnect = true;
    if (session.tracksClones) {
      yield* rpc[WS_METHODS.subscribeProjectClones]({}).pipe(
        Stream.runForEach((clones) =>
          Effect.sync(() => {
            const next = new Map<string, string>();
            const changed: string[] = [];
            for (const clone of clones) {
              if (clone.phase !== "done") continue;
              const version = clone.endedAt ?? clone.startedAt;
              next.set(clone.projectId, version);
              if (completedClones.get(clone.projectId) !== version) changed.push(clone.projectId);
            }
            completedClones = next;
            if (changed.length) refreshFavicons(changed);
          }),
        ),
        Effect.ignore,
        Effect.forkScoped,
      );
    }
    const feed = rpc[ORCHESTRATION_V2_WS_METHODS.subscribeShell]({
      afterSequence: sequence,
      ...(identities && { knownProjectIdentities: identities }),
      ...(session.shellThreadFamilies && { omitIdleSubagents: true }),
      httpSnapshotFallback: true,
      requestCompletionMarker: true,
    });
    yield* feed.pipe(
      Stream.runForEach((item) =>
        Effect.gen(function* () {
          if (item.kind === "snapshot-required") {
            needSnapshot = true;
            return yield* Effect.fail("Shell snapshot required");
          }
          if (item.kind === "synchronized") {
            if (retryFavicons) refreshFavicons();
            retryFavicons = false;
            if (activeRpc !== rpc && !signal.aborted) onSynchronized(true);
            activeRpc = rpc;
            fileActions.reconcile();
            git.connect(rpc);
            currentVisit()?.reconnect();
            failures = 0;
            return;
          }
          // Enrichment snapshots contain repository identities only and do not advance replay.
          if (item.kind === "snapshot" && item.resolvedRepositoryIdentityRoots !== undefined)
            return;
          identities = undefined;
          if (item.kind === "snapshot") {
            sequence = item.snapshot.snapshotSequence;
            failures = 0;
            replace(workspaceRows(item.snapshot, session, scratchRoot));
            return;
          }
          if (item.sequence <= sequence) return;
          sequence = item.sequence;
          if (item.kind === "thread.updated") {
            const active = item.location === "active" && visibleThread(item.thread);
            changeRow(
              threads,
              dirtyThreads,
              item.thread.id,
              active ? threadSummary(item.thread, session) : undefined,
            );
            const visit = active ? visitInput(session.environmentId, item.thread) : null;
            if (visit) visitRows.set(item.thread.id, visit);
            else visitRows.delete(item.thread.id);
            const current = currentVisit();
            if (current?.id === item.thread.id) current.update(visit);
          } else if (item.kind === "project.updated") {
            if (item.project.scripts.length)
              legacyScripts.set(item.project.id, item.project.scripts);
            else legacyScripts.delete(item.project.id);
            changeRow(
              projects,
              dirtyProjects,
              item.project.id,
              projectSummary(item.project, scratchRoot),
            );
            scripts.publish();
          } else if (item.kind === "thread.removed") {
            visitRows.delete(item.threadId);
            const current = currentVisit();
            if (current?.id === item.threadId) current.update(null);
            changeRow(threads, dirtyThreads, item.threadId, undefined);
          } else {
            changeRow(projects, dirtyProjects, item.projectId, undefined);
            legacyScripts.delete(item.projectId);
            scripts.publish();
          }
          fileActions.reconcile();
          flush();
        }),
      ),
    );
    return yield* Effect.fail("Workspace stream ended");
  });
  void Effect.runPromise(
    subscribe().pipe(
      Effect.scoped,
      Effect.retry({ schedule: Schedule.spaced("1 second"), while: () => ++failures <= 3 }),
      Effect.provide(session.http),
    ),
    { signal },
  ).catch(() => {
    if (!signal.aborted) paused();
  });
  return {
    initialLanding,
    selectDraftModels: draftModels.select,
    selectBanner: draftModels.selectBanner,
    git,
    scripts,
    fileActions,
    selectScriptTarget(target: typeof scriptTarget) {
      scriptTarget = target;
      fileActions.reconcile();
    },
    refreshProvider: draftModels.refreshProvider,
    draftConfig: draftModels.config,
    activeRpc: () => activeRpc,
    projectFavicon(id: string) {
      const row = projects.get(id);
      return row?.favicon && !row.archived
        ? {
            _tag: "project-favicon" as const,
            cwd: row.path,
            ...(row.favicon.path && { path: row.favicon.path }),
          }
        : undefined;
    },
    publishThrough(committed: number) {
      requestedSequence = committed;
      flush();
    },
    nextPinOrderKey() {
      const keys = Array.from(threads.values(), (row) => (row.pinned ? row.order_key : null));
      if (lastPin && lastPin.sequence > sequence) keys.push(lastPin.key);
      return topOfPinnedRunOrderKey(keys) ?? undefined;
    },
    pinCommitted(key: string, committed: number) {
      lastPin = { key, sequence: committed };
    },
    searchComposerFiles: Effect.fn("nativeDesktop.searchComposerFiles")(function* (
      draft: boolean,
      id: string,
      root: string,
      query: string,
    ) {
      const checkout = () => {
        const thread = draft ? undefined : threads.get(id);
        const project = projects.get(draft ? id : (thread?.project ?? ""));
        return project && !project.archived ? (thread?.worktree ?? project.path) : undefined;
      };
      const rpc = activeRpc;
      if (!session.canBrowse || !session.canOperate || !rpc || checkout() !== root) return [];
      const { entries } = yield* rpc[WS_METHODS.projectsSearchEntries]({
        cwd: root,
        query: query
          .trim()
          .slice(0, 256)
          .replace(/[\uD800-\uDBFF]$/, ""),
        limit: 80,
      });
      if (activeRpc !== rpc || checkout() !== root) return [];
      return entries.map(({ path, kind }) => ({
        path,
        kind,
        link: serializeComposerFileLink(path),
      }));
    }),
    fileRoot(id: string) {
      const thread = threads.get(id);
      return thread && (thread.worktree ?? projects.get(thread.project)?.path);
    },
    hasThread(id: string) {
      return threads.has(id);
    },
    visitSource(id: string) {
      return visitRows.get(id) ?? null;
    },
    seen(id: string, at: string) {
      const source = visitRows.get(id)?.legacy;
      if (!session.legacyVisits?.mark(id, at) || !source) return;
      const row = threadSummary(source, session);
      changeRow(threads, dirtyThreads, id, row);
      flush();
    },
    async launched(row: ThreadRow, committed: number | undefined) {
      if (committed === undefined) {
        const fresh = await readWorkspace(session, signal);
        if (fresh.sequence >= sequence) adopt(fresh);
      }
      const current = threads.get(row.id);
      if (current) return current;
      if (committed === undefined || sequence >= committed) return null;
      // The receipt reaches Rune before its shell event. Track it so the next removal is delivered.
      threads.set(row.id, row);
      return row;
    },
    opened(id: string, at: number) {
      const row = threads.get(id);
      if (!row || row.recent_at >= at) return;
      changeRow(threads, dirtyThreads, id, { ...row, recent_at: at });
      flush();
    },
    handled: () => {
      waiting = false;
      flush();
    },
  };
}
