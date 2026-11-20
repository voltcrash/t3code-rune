import {
  applyOrchestrationV2ProjectionEvent,
  getProjectionArrayReplacementIndex,
} from "@t3tools/client-runtime/state/orchestration-v2-projection";
import { activeThreadRun } from "@t3tools/client-runtime/state/thread-execution";
import { mergeOlderHistoryIntoProjection } from "@t3tools/client-runtime/state/thread-history-merge";
import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  ORCHESTRATION_V2_WS_METHODS,
  ThreadId,
  type OrchestrationV2DomainEvent,
  type OrchestrationV2ProjectedTurnItem,
  type OrchestrationV2ThreadBoundedSnapshot,
  type OrchestrationV2ThreadProjection,
  type OrchestrationV2ThreadStreamItem,
} from "@t3tools/contracts";
import { boundedSnapshotProjection } from "@t3tools/shared/orchestrationV2BoundedSnapshot";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import * as Schedule from "effect/Schedule";
import * as Schema from "effect/Schema";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { headersFor, type Session } from "./projects.ts";

function message(row: OrchestrationV2ProjectedTurnItem) {
  const item = row.item;
  if (item.type !== "user_message" && item.type !== "assistant_message") return null;
  return {
    key: JSON.stringify([row.sourceThreadId, row.sourceItemId]),
    role: item.type === "user_message" ? "user" : "assistant",
    body: item.text,
    attachments: (item.attachments ?? []).map((a) => a.name),
  };
}
type Row = NonNullable<ReturnType<typeof message>>;
const messages = (items: readonly OrchestrationV2ProjectedTurnItem[]) =>
  items.flatMap((row) => message(row) ?? []);
type Window = {
  projection: OrchestrationV2ThreadProjection;
  activeRun: string | null;
  sequence: number;
  watermark: number | null;
  rows: Row[];
  indices: Map<string, number>;
  cursor: string | null;
  earlier_pages: number;
};
type Update =
  | { kind: "splice"; index: number; remove: number; rows: Row[] }
  | { kind: "append"; index: number; key: string; text: string }
  | ({ kind: "window" } & ReturnType<typeof windowReply>)
  | { kind: "status"; state: "live" | "reconnecting" | "paused" }
  | { kind: "run"; run: string | null }
  | { kind: "deleted" };

const indexRows = (rows: Row[]) => new Map(rows.map((row, i) => [row.key, i]));
function windowRows(state: Window) {
  state.rows = messages(state.projection.visibleTurnItems);
  state.indices = indexRows(state.rows);
}
function earlier(
  state: Window,
  page: {
    items: readonly OrchestrationV2ProjectedTurnItem[];
    hasMoreHistory: boolean;
    nextCursor: string | null;
  },
) {
  state.projection = mergeOlderHistoryIntoProjection(state.projection, page.items);
  state.cursor = page.hasMoreHistory ? page.nextCursor : null;
  state.earlier_pages += 1;
}
const readPage = Effect.fn("nativeDesktop.readHistoryPage")(function* (
  session: Session,
  id: string,
  cursor: string,
) {
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  return yield* api.threadHistoryPage({
    headers: headersFor(session),
    params: { threadId: ThreadId.make(id) },
    query: { cursor, view: "conversation", deferWork: "1" },
  });
});
type Snapshot =
  | OrchestrationV2ThreadBoundedSnapshot
  | Extract<OrchestrationV2ThreadStreamItem, { kind: "snapshot" }>;
const expandedWindow = Effect.fn("nativeDesktop.expandedWindow")(function* (
  session: Session,
  snapshot: Snapshot,
  oldest: string | undefined,
  depth: number,
) {
  const projection = boundedSnapshotProjection(snapshot);
  const state: Window = {
    projection,
    activeRun: activeThreadRun(projection)?.id ?? null,
    sequence: snapshot.snapshotSequence,
    watermark: snapshot.latestLocalTurnOrdinal ?? null,
    cursor: snapshot.hasMoreHistory ? (snapshot.historyCursor ?? null) : null,
    earlier_pages: 0,
    rows: [],
    indices: new Map(),
  };
  let lastPage = projection.visibleTurnItems;
  while (
    state.cursor &&
    state.earlier_pages < depth &&
    !lastPage.some((row) => oldest !== undefined && message(row)?.key === oldest)
  ) {
    const page = yield* readPage(session, projection.thread.id, state.cursor);
    earlier(state, page);
    lastPage = page.items;
  }
  windowRows(state);
  return state;
});

type ReadRequest = {
  thread: string;
  cursor?: string;
  sequence?: number;
  oldest?: string;
  earlier_pages: number;
};
const loadWindow = Effect.fn("nativeDesktop.loadWindow")(function* (
  session: Session,
  request: ReadRequest,
  previous?: Window,
) {
  if (request.cursor && previous) {
    if (previous.cursor !== request.cursor) return previous;
    const page = yield* readPage(session, request.thread, request.cursor);
    earlier(previous, page);
    windowRows(previous);
    return previous;
  }
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const snapshot = yield* api.threadBoundedSnapshot({
    headers: headersFor(session),
    params: { threadId: ThreadId.make(request.thread) },
    query: { deferWork: "1", dedupeBodies: "1", compactTurnItems: "1" },
  });
  if (previous && previous.sequence === snapshot.snapshotSequence) return previous;
  return yield* expandedWindow(session, snapshot, request.oldest, request.earlier_pages);
});
export function readWindow(session: Session, request: ReadRequest, previous?: Window) {
  return Effect.runPromise(
    loadWindow(session, request, previous).pipe(
      Effect.timeout("20 seconds"),
      Effect.provide(session.http),
    ),
    { signal: session.signal },
  );
}
export function windowReply(state: Window) {
  return {
    rows: state.rows,
    active_run: state.activeRun,
    cursor: state.cursor,
    earlier_pages: state.earlier_pages,
    sequence: state.sequence,
  };
}

function sameRow(a: Row, b: Row) {
  return (
    a.key === b.key &&
    a.role === b.role &&
    a.body === b.body &&
    a.attachments.length === b.attachments.length &&
    a.attachments.every((value, i) => value === b.attachments[i])
  );
}
function applyEvent(state: Window, event: OrchestrationV2DomainEvent): Update | undefined {
  const previous = state.projection;
  const next = applyOrchestrationV2ProjectionEvent(
    previous,
    event,
    state.cursor ? { partialTimeline: true, latestLocalTurnOrdinal: state.watermark } : undefined,
  );
  if (!next) return;
  state.projection = next;
  if (event.type === "turn-item.updated")
    state.watermark = Math.max(state.watermark ?? 0, event.payload.ordinal);
  if (previous.visibleTurnItems === next.visibleTurnItems) return;
  const changed = getProjectionArrayReplacementIndex(
    previous.visibleTurnItems,
    next.visibleTurnItems,
  );
  if (changed !== undefined) {
    const oldItem = previous.visibleTurnItems[changed]!;
    const newItem = next.visibleTurnItems[changed]!;
    const row = message(newItem);
    const key = JSON.stringify([oldItem.sourceThreadId, oldItem.sourceItemId]);
    const i = state.indices.get(key);
    if (!row && i === undefined) return;
    if (row && i !== undefined && row.key === key) {
      const old = state.rows[i]!;
      if (sameRow(old, row)) return;
      state.rows[i] = row;
      if (
        row.role === old.role &&
        row.attachments.length === 0 &&
        old.attachments.length === 0 &&
        row.body.startsWith(old.body)
      ) {
        return { kind: "append", index: i, key, text: row.body.slice(old.body.length) };
      }
      return { kind: "splice", index: i, remove: 1, rows: [row] };
    }
  }
  const old = state.rows;
  windowRows(state);
  const rows = state.rows;
  let start = 0;
  let end = 0;
  while (start < old.length && start < rows.length && sameRow(old[start]!, rows[start]!)) start++;
  while (
    end < old.length - start &&
    end < rows.length - start &&
    sameRow(old[old.length - 1 - end]!, rows[rows.length - 1 - end]!)
  )
    end++;
  const remove = old.length - start - end;
  if (remove === 0 && rows.length === start + end) return;
  return { kind: "splice", index: start, remove, rows: rows.slice(start, rows.length - end) };
}

class LiveStreamEnded extends Schema.TaggedError<LiveStreamEnded>()("LiveStreamEnded", {}) {}
export function followWindow(
  session: Session,
  state: Window,
  signal: AbortSignal,
  emit: (update: Update) => void,
) {
  const id = state.projection.thread.id;
  let consecutiveFailures = 0;
  const attempt = Effect.fn("nativeDesktop.followWindow")(function* () {
    const rpc = yield* openEnvironmentRpc(session);
    const feed = rpc[ORCHESTRATION_V2_WS_METHODS.subscribeThread]({
      threadId: id,
      afterSequence: state.sequence,
      requestCompletionMarker: true,
      acceptBoundedSnapshot: true,
      acceptDeferredWork: true,
      acceptCompactTurnItems: true,
      acceptDedupedBodies: true,
    });
    yield* feed.pipe(
      Stream.runForEach((item) =>
        Effect.gen(function* () {
          if (item.kind === "synchronized") {
            consecutiveFailures = 0;
            emit({ kind: "status", state: "live" });
            return;
          }
          if (item.kind === "snapshot") {
            const fresh = yield* expandedWindow(
              session,
              item,
              state.rows[0]?.key,
              state.earlier_pages,
            );
            Object.assign(state, fresh);
            emit({ kind: "window", ...windowReply(state) });
            return;
          }
          if (item.sequence <= state.sequence) return;
          if (item.kind === "unknown-event") {
            state.sequence = item.sequence;
            return;
          }
          if (item.event.type === "thread.deleted" || item.event.type === "thread.archived") {
            emit({ kind: "deleted" });
            return;
          }
          const previousRuns = state.projection.runs;
          const update = applyEvent(state, item.event);
          state.sequence = item.sequence;
          if (previousRuns !== state.projection.runs) {
            const run = activeThreadRun(state.projection)?.id ?? null;
            if (state.activeRun !== run) {
              state.activeRun = run;
              emit({ kind: "run", run });
            }
          }
          if (update) emit(update);
        }),
      ),
    );
    return yield* new LiveStreamEnded();
  });
  return Effect.runPromise(
    attempt().pipe(
      Effect.scoped,
      Effect.tapError(() => Effect.sync(() => emit({ kind: "status", state: "reconnecting" }))),
      Effect.retry({
        schedule: Schedule.spaced("1 second"),
        while: () => ++consecutiveFailures <= 2,
      }),
      Effect.provide(session.http),
    ),
    { signal },
  ).catch(() => {
    if (!signal.aborted) emit({ kind: "status", state: "paused" });
  });
}
