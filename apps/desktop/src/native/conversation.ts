import {
  buildProposedPlanMarkdownFilename,
  proposedPlanBodyStart,
  proposedPlanCanCollapse,
  proposedPlanPreviewEnd,
  proposedPlanTitle,
} from "@t3tools/shared/proposedPlanText";
import {
  applyOrchestrationV2ProjectionEvent,
  getProjectionArrayReplacementIndex,
} from "@t3tools/client-runtime/state/orchestration-v2-projection";
import { taskListLabel } from "@t3tools/client-runtime/work-log/presentation";
import { turnItemIsWorkspacePreparation } from "@t3tools/client-runtime/state/turn-item-presentation";
import {
  commandHighlightLanguage,
  withVisibleControlCharacters,
  commandProgramName,
} from "@t3tools/client-runtime/work-log/command-label";
import {
  activeThreadRun,
  deriveRunlessWorkStartedAt,
} from "@t3tools/client-runtime/state/thread-execution";
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
import { pendingRequests } from "./runtimeRequests.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { headersFor, type Session } from "./projects.ts";

import { filePreviewKind, type FilePreviewKind } from "@t3tools/shared/filePreview";
import { formatBytes } from "@t3tools/shared/formatBytes";
import { imageKey } from "./images.ts";
import { toolRow } from "./toolDetail.ts";
import { fileChangeRow } from "./fileChange.ts";
import * as NodeUtil from "node:util";
import { nativeTurnFolds } from "./turnFolds.ts";

type Row = {
  key: string;
  role:
    | "command"
    | "reasoning"
    | "tasks"
    | "file"
    | "tool"
    | "plan"
    | "sent-message"
    | "user"
    | "assistant";
  body: string;
  attachments: Array<{
    name: string;
    image?: string;
    file?: {
      id: string;
      bytes: number;
      size: string;
      extension?: string;
      pasted: boolean;
      kind: FilePreviewKind;
    };
  }>;
  caption?: string;
  plan?: { filename: string; start: number; collapsible: boolean; preview_end?: number };
  language?: string;
  exit_code?: number | undefined;
  failed?: boolean | undefined;
  tool?: ReturnType<typeof toolRow>["tool"];
  file?: ReturnType<typeof fileChangeRow>["file"];
};

function timelineRow(row: OrchestrationV2ProjectedTurnItem, root?: string): Row | null {
  const item = row.item;
  const key = JSON.stringify([row.sourceThreadId, row.sourceItemId]);
  if (item.type === "proposed_plan") {
    const body = item.markdown;
    const start = proposedPlanBodyStart(body);
    const title = proposedPlanTitle(body);
    const collapsible = proposedPlanCanCollapse(body);
    const previewEnd = collapsible ? proposedPlanPreviewEnd(body.slice(start), 10) : undefined;
    return {
      key,
      role: "plan",
      body,
      attachments: [],
      ...(title ? { caption: title } : {}),
      plan: {
        filename: buildProposedPlanMarkdownFilename(body),
        start: Buffer.byteLength(body.slice(0, start)),
        collapsible,
        ...(previewEnd !== undefined
          ? { preview_end: Buffer.byteLength(body.slice(start, start + previewEnd)) }
          : {}),
      },
    };
  }
  if (item.type === "dynamic_tool" || item.type === "file_search" || item.type === "web_search")
    return toolRow(key, row, root);
  if (item.type === "file_change") return fileChangeRow(key, item, root);
  if (item.type === "command_execution" && !turnItemIsWorkspacePreparation(item)) {
    const failed =
      item.status === "failed" ||
      (item.exitCode !== undefined ? item.exitCode !== 0 : item.outputIndicatesFailure);
    const verb =
      item.status === "interrupted" || item.status === "cancelled"
        ? "Stopped"
        : failed
          ? "Failed"
          : item.status === "completed"
            ? "Ran"
            : "Running";
    const body = withVisibleControlCharacters(item.input.trim());
    return {
      key,
      role: "command",
      body,
      language: commandHighlightLanguage(body),
      attachments: [],
      caption: verb + " " + (commandProgramName(item.input) ?? "command"),
      exit_code: item.exitCode,
      failed: verb === "Failed",
    };
  }
  if (item.type === "reasoning" && item.text) {
    return {
      key,
      role: "reasoning",
      body: item.text,
      caption: item.streaming ? "Thinking" : "Thought",
      attachments: [],
    };
  }
  if (item.type === "todo_list") {
    const body = item.steps
      .map((step) => {
        const mark = step.status === "completed" ? "✓" : step.status === "running" ? "◉" : "○";
        return mark + " " + step.text;
      })
      .join("\n");
    return { key, role: "tasks", body, caption: taskListLabel(item.steps), attachments: [] };
  }
  if (item.type !== "user_message" && item.type !== "assistant_message") return null;
  return {
    key,
    role: item.type === "user_message" ? "user" : "assistant",
    body: item.text,
    attachments: (item.attachments ?? []).map((a) => {
      const name = a.name;
      if (a.type === "image") {
        return { name, image: imageKey({ _tag: "attachment", attachmentId: a.id }) };
      }
      if (a.type !== "file") return { name };
      const extension = /\.([a-z0-9]{1,8})$/i.exec(name)?.[1]?.toUpperCase();
      return {
        name,
        file: {
          id: a.id,
          kind: filePreviewKind(a),
          bytes: a.sizeBytes,
          size: formatBytes(a.sizeBytes),
          ...(extension ? { extension } : {}),
          pasted: "source" in a && a.source?._tag === "pasted-text",
        },
      };
    }),
  };
}

type Folds = ReturnType<typeof nativeTurnFolds>;
type Window = {
  folds: Folds;
  projectRoot: string | undefined;
  projection: OrchestrationV2ThreadProjection;
  activeRun: string | null;
  runlessWorkActive: boolean;
  requests: ReturnType<typeof pendingRequests>;
  sequence: number;
  watermark: number | null;
  rows: Row[];
  indices: Map<string, number>;
  cursor: string | null;
  earlier_pages: number;
};
type Update =
  | { kind: "folds"; folds: Folds }
  | { kind: "splice"; index: number; remove: number; rows: Row[]; folds?: Folds }
  | { kind: "append"; index: number; key: string; text: string }
  | ({ kind: "window" } & ReturnType<typeof windowReply>)
  | { kind: "status"; state: "live" | "reconnecting" | "paused" }
  | { kind: "run"; run: string | null }
  | ({ kind: "requests" } & Window["requests"])
  | { kind: "deleted" };

const indexRows = (rows: Row[]) => new Map(rows.map((row, i) => [row.key, i]));
function windowRows(state: Window) {
  state.rows = state.projection.visibleTurnItems.flatMap(
    (row) => timelineRow(row, state.projection.thread.worktreePath ?? state.projectRoot) ?? [],
  );
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
    query: { cursor, view: "activity" },
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
  projectRoot?: string,
) {
  const projection = boundedSnapshotProjection(snapshot);
  const state: Window = {
    projectRoot,
    projection,
    requests: { approvals: [], questions: [] },
    activeRun: activeThreadRun(projection)?.id ?? null,
    runlessWorkActive: deriveRunlessWorkStartedAt(projection) !== null,
    sequence: snapshot.snapshotSequence,
    watermark: snapshot.latestLocalTurnOrdinal ?? null,
    cursor: snapshot.hasMoreHistory ? (snapshot.historyCursor ?? null) : null,
    earlier_pages: 0,
    folds: [],
    rows: [],
    indices: new Map(),
  };
  let lastPage = projection.visibleTurnItems;
  while (
    state.cursor &&
    state.earlier_pages < depth &&
    !lastPage.some(
      (row) =>
        oldest !== undefined && JSON.stringify([row.sourceThreadId, row.sourceItemId]) === oldest,
    )
  ) {
    const page = yield* readPage(session, projection.thread.id, state.cursor);
    earlier(state, page);
    lastPage = page.items;
  }
  windowRows(state);
  state.folds = nativeTurnFolds(state.projection, state.indices);
  state.requests = pendingRequests(state.projection);
  return state;
});

type ReadRequest = {
  thread: string;
  cursor?: string;
  sequence?: number;
  oldest?: string;
  earlier_pages: number;
  projectRoot?: string;
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
    previous.folds = nativeTurnFolds(previous.projection, previous.indices);
    previous.requests = pendingRequests(previous.projection);
    return previous;
  }
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  const snapshot = yield* api.threadBoundedSnapshot({
    headers: headersFor(session),
    params: { threadId: ThreadId.make(request.thread) },
    query: { dedupeBodies: "1", compactTurnItems: "1" },
  });
  if (previous && previous.sequence === snapshot.snapshotSequence) return previous;
  return yield* expandedWindow(
    session,
    snapshot,
    request.oldest,
    request.earlier_pages,
    request.projectRoot,
  );
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
    folds: state.folds,
    active_run: state.activeRun,
    ...state.requests,
    cursor: state.cursor,
    earlier_pages: state.earlier_pages,
    sequence: state.sequence,
  };
}

function samePlan(a: Row, b: Row) {
  return (
    a.plan?.start === b.plan?.start &&
    a.plan?.collapsible === b.plan?.collapsible &&
    a.plan?.preview_end === b.plan?.preview_end
  );
}
function sameRow(a: Row, b: Row) {
  return (
    a.key === b.key &&
    a.role === b.role &&
    a.body === b.body &&
    a.caption === b.caption &&
    samePlan(a, b) &&
    a.exit_code === b.exit_code &&
    a.failed === b.failed &&
    a.tool?.revision === b.tool?.revision &&
    a.tool?.action === b.tool?.action &&
    a.file?.revision === b.file?.revision &&
    a.file?.added === b.file?.added &&
    a.file?.removed === b.file?.removed &&
    a.attachments.length === b.attachments.length &&
    a.attachments.every(
      (value, i) =>
        value.name === b.attachments[i]?.name &&
        value.image === b.attachments[i]?.image &&
        value.file?.id === b.attachments[i]?.file?.id &&
        value.file?.kind === b.attachments[i]?.file?.kind &&
        value.file?.bytes === b.attachments[i]?.file?.bytes &&
        value.file?.pasted === b.attachments[i]?.file?.pasted,
    )
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
    const row = timelineRow(newItem, state.projection.thread.worktreePath ?? state.projectRoot);
    const key = JSON.stringify([oldItem.sourceThreadId, oldItem.sourceItemId]);
    const i = state.indices.get(key);
    if (!row && i === undefined) return;
    if (row && i !== undefined && row.key === key) {
      const old = state.rows[i]!;
      if (sameRow(old, row)) return;
      state.rows[i] = row;
      if (
        !row.tool &&
        !row.file &&
        row.role !== "command" &&
        row.role === old.role &&
        row.caption === old.caption &&
        samePlan(old, row) &&
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
  inspectedItemChanged?: (threadId: string, itemId: string) => void,
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
              state.projectRoot,
            );
            if (
              fresh.projection.thread.deletedAt !== null ||
              fresh.projection.thread.archivedAt !== null
            ) {
              emit({ kind: "deleted" });
              return;
            }
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
          const before = state.projection;
          const previousRuns = before.runs;
          const previousRequests = state.projection.runtimeRequests;
          const update = applyEvent(state, item.event);
          state.sequence = item.sequence;
          if (previousRuns !== state.projection.runs) {
            const run = activeThreadRun(state.projection)?.id ?? null;
            if (state.activeRun !== run) {
              state.activeRun = run;
              emit({ kind: "run", run });
            }
          }
          if (
            previousRequests !== state.projection.runtimeRequests ||
            (item.event.type === "turn-item.updated" &&
              (item.event.payload.type === "approval_request" ||
                item.event.payload.type === "user_input_request"))
          ) {
            state.requests = pendingRequests(state.projection);
            emit({ kind: "requests", ...state.requests });
          }
          const wasRunlessActive = state.runlessWorkActive;
          if (before.nodes !== state.projection.nodes || before.thread !== state.projection.thread)
            state.runlessWorkActive = deriveRunlessWorkStartedAt(state.projection) !== null;
          const liveItem =
            item.event.type === "turn-item.updated" &&
            (state.activeRun !== null || (state.runlessWorkActive && update?.kind === "append")) &&
            item.event.payload.runId === state.activeRun;
          const structureChanged =
            before.runs !== state.projection.runs ||
            before.attempts !== state.projection.attempts ||
            before.subagents !== state.projection.subagents ||
            wasRunlessActive !== state.runlessWorkActive;
          const rowsChanged = before.visibleTurnItems !== state.projection.visibleTurnItems;
          let changedFolds: Folds | undefined;
          if (
            structureChanged ||
            (rowsChanged &&
              (!liveItem ||
                (update?.kind === "splice" &&
                  state.folds.some(
                    (fold) =>
                      update.index <= fold.at || update.index < (fold.ranges.at(-1)?.[1] ?? 0),
                  ))))
          ) {
            const folds = nativeTurnFolds(state.projection, state.indices);
            if (!NodeUtil.isDeepStrictEqual(state.folds, folds)) {
              state.folds = changedFolds = folds;
            }
          }
          if (update?.kind === "splice" && changedFolds) emit({ ...update, folds: changedFolds });
          else {
            if (update) emit(update);
            if (changedFolds) emit({ kind: "folds", folds: changedFolds });
          }
          if (
            item.event.type === "turn-item.updated" &&
            (item.event.payload.type === "command_execution" ||
              item.event.payload.type === "dynamic_tool" ||
              item.event.payload.type === "web_search" ||
              item.event.payload.type === "file_search")
          ) {
            inspectedItemChanged?.(item.event.payload.threadId, item.event.payload.id);
          }
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
