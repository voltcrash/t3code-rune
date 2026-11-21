import { visitInput, type trackVisibleThread } from "./visits.ts";
import { ORCHESTRATION_V2_WS_METHODS } from "@t3tools/contracts";
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
  currentVisit: () => ReturnType<typeof trackVisibleThread> | undefined,
) {
  let { sequence, identities, scratchRoot, visitRows } = initial;
  const projects = rowsById(initial.view.projects);
  const threads = rowsById(initial.view.threads);
  const dirtyProjects = new Set<string>();
  const dirtyThreads = new Set<string>();
  let scratchAvailable = initial.view.scratch_available;
  let scratchChanged = false;
  let waiting = false;
  let scheduled = false;
  const flush = () => {
    if (
      scheduled ||
      waiting ||
      signal.aborted ||
      (!scratchChanged && !dirtyThreads.size && !dirtyProjects.size)
    )
      return;
    scheduled = true;
    void Effect.runPromise(
      Effect.sleep("16 millis").pipe(
        Effect.andThen(
          Effect.sync(() => {
            scheduled = false;
            const p = takeChanges(projects, dirtyProjects);
            const t = takeChanges(threads, dirtyThreads);
            waiting = true;
            emit({
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
  function replace(fresh: Pick<Loaded, "view" | "visitRows">) {
    const { view } = fresh;
    visitRows = fresh.visitRows;
    reconcileRows(projects, dirtyProjects, view.projects);
    reconcileRows(threads, dirtyThreads, view.threads);
    scratchChanged ||= view.scratch_available !== scratchAvailable;
    scratchAvailable = view.scratch_available;
    const active = currentVisit();
    if (active) active.update(visitRows.get(active.id) ?? null);
    flush();
  }
  function adopt(fresh: Loaded) {
    sequence = fresh.sequence;
    identities = fresh.identities;
    scratchRoot = fresh.scratchRoot;
    if (fresh.clockOffset !== undefined) onClock(fresh.clockOffset);
    replace(fresh);
  }
  let sampleOnConnect = initial.clockOffset === undefined;
  let failures = 0;
  let needSnapshot = false;
  const subscribe = Effect.fn("nativeDesktop.followWorkspace")(function* () {
    if (needSnapshot) {
      const fresh = yield* loadWorkspace(session).pipe(Effect.timeout("20 seconds"));
      adopt(fresh);
      needSnapshot = false;
    }
    const rpc = yield* openEnvironmentRpc(session);
    if (sampleOnConnect) {
      yield* sampleDesktopConfig(rpc, session).pipe(
        Effect.timeout("3 seconds"),
        Effect.tap(({ clockOffset }) =>
          Effect.sync(() => {
            if (clockOffset !== undefined) onClock(clockOffset);
          }),
        ),
        Effect.ignore,
        Effect.forkScoped,
      );
    }
    sampleOnConnect = true;
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
            changeRow(
              projects,
              dirtyProjects,
              item.project.id,
              projectSummary(item.project, scratchRoot),
            );
          } else if (item.kind === "thread.removed") {
            visitRows.delete(item.threadId);
            const current = currentVisit();
            if (current?.id === item.threadId) current.update(null);
            changeRow(threads, dirtyThreads, item.threadId, undefined);
          } else {
            changeRow(projects, dirtyProjects, item.projectId, undefined);
          }
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
