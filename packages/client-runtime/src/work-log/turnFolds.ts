import * as DateTime from "effect/DateTime";
import { mFallback, msg } from "gt-i18n";
import type { MFunctionType } from "gt-i18n/types";
import { RunId, type RunAttemptId } from "@t3tools/contracts";
import { formatDuration } from "@t3tools/shared/orchestrationTimingCopy";
import { isLiveSubagentTurnItem } from "../state/subagentRuntime.ts";
import type { ThreadRunSummary } from "../state/shell.ts";
import { type TimelineEntry, timelineEntryIsPersistentResourceCard } from "./timeline.ts";
import { workEntryDisplayIndicatesToolFailure } from "./presentation.ts";
export function timelineEntryRunId(entry: TimelineEntry): RunId | null {
  if (entry.kind === "message") {
    return entry.message.role === "assistant" ? (entry.message.runId ?? null) : null;
  }
  if (entry.kind === "proposed-plan") {
    return entry.proposedPlan.runId;
  }
  if (entry.kind === "html-render" || entry.kind === "mcp-app" || entry.kind === "sent-message") {
    return entry.runId;
  }
  return entry.kind === "work" ? (entry.entry.runId ?? null) : null;
}

function computeElapsedMs(startIso: string, endIso: string): number | null {
  const start = Date.parse(startIso);
  const end = Date.parse(endIso);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
  return Math.max(0, end - start);
}

function maxIsoTimestamp(a: string | null, b: string | null): string | null {
  if (a === null) return b;
  if (b === null) return a;
  const aMs = Date.parse(a);
  const bMs = Date.parse(b);
  if (!Number.isFinite(aMs)) return b;
  if (!Number.isFinite(bMs)) return a;
  return bMs > aMs ? b : a;
}

export type TimelineLatestRun = Pick<
  ThreadRunSummary,
  "runId" | "status" | "startedAt" | "completedAt" | "turnEnded"
>;

export function deriveTerminalAssistantMessageIds(timelineEntries: ReadonlyArray<TimelineEntry>) {
  const lastAssistantMessageIdByResponseKey = new Map<string, string>();
  let nullTurnResponseIndex = 0;

  for (const timelineEntry of timelineEntries) {
    if (timelineEntry.kind !== "message") {
      continue;
    }
    const { message } = timelineEntry;
    if (message.role === "user") {
      nullTurnResponseIndex += 1;
      continue;
    }
    if (message.role !== "assistant") {
      continue;
    }

    const responseKey = message.runId
      ? `turn:${message.runId}`
      : `unkeyed:${nullTurnResponseIndex}`;
    lastAssistantMessageIdByResponseKey.set(responseKey, message.id);
  }

  return new Set(lastAssistantMessageIdByResponseKey.values());
}

export interface TurnFold {
  runId: RunId;
  anchorEntryId: string;
  createdAt: string;
  hiddenEntryIds: ReadonlySet<string>;
  /** Every entry in the segment, including the cards a fold never hides. */
  entryIds: ReadonlySet<string>;
  label: string;
}

// Only an explicit final phase proves a reply cut short by steering was an answer.
export function retainedAssistantReplyEntryIds(
  timelineEntries: ReadonlyArray<TimelineEntry>,
): ReadonlySet<string> {
  const replyEntryIds = new Set<string>();
  const lastOutputByAttemptId = new Map<RunAttemptId, TimelineEntry>();
  for (const entry of timelineEntries) {
    if (
      entry.kind === "message" &&
      entry.projectedItem?.item.type === "assistant_message" &&
      entry.projectedItem.item.phase === "final_answer"
    )
      replyEntryIds.add(entry.id);

    if (
      entry.attempt?.status !== "superseded" ||
      (entry.kind === "message" && entry.message.role === "user") ||
      (entry.kind === "work" &&
        (entry.entry.itemType === "system_notice" || entry.entry.itemType === "notification")) ||
      (entry.kind !== "message" && entry.kind !== "work")
    ) {
      continue;
    }
    const previous = lastOutputByAttemptId.get(entry.attempt.id);
    const deferredEnd =
      previous?.kind === "work"
        ? previous.entry.projectedItem?.item.deferredWork?.lastOrdinal
        : undefined;
    const ordinal =
      entry.kind === "work"
        ? entry.entry.projectedItem?.item.ordinal
        : entry.kind === "message"
          ? entry.projectedItem?.item.ordinal
          : undefined;
    if (deferredEnd !== undefined && ordinal !== undefined && deferredEnd > ordinal) continue;
    lastOutputByAttemptId.set(entry.attempt.id, entry);
  }
  for (const entry of lastOutputByAttemptId.values()) {
    const item = entry.kind === "message" ? entry.projectedItem?.item : undefined;
    const supersededAt = entry.attempt?.completedAt ?? null;
    if (
      entry.kind === "message" &&
      !entry.message.streaming &&
      item?.type === "assistant_message" &&
      item.phase === undefined &&
      item.status === "completed" &&
      item.completedAt !== null &&
      supersededAt !== null &&
      DateTime.isLessThan(item.completedAt, supersededAt)
    ) {
      replyEntryIds.add(entry.id);
    }
  }
  return replyEntryIds;
}

// After send, the previous turn stays active until the server creates its replacement.
export function deriveUnsettledRunId(
  latestRun: TimelineLatestRun | null,
  runningRunId: RunId | null = null,
): RunId | null {
  if (runningRunId !== null) return runningRunId;
  if (!latestRun) {
    return null;
  }
  const isSettled =
    latestRun.completedAt !== null &&
    latestRun.status !== "running" &&
    latestRun.status !== "starting" &&
    latestRun.status !== "waiting";
  return isSettled ? null : latestRun.runId;
}

// Children can outlive their launching turn; keep cards visible while the footer counts them.
export function liveSubagentCardEntryIds(
  entries: ReadonlyArray<TimelineEntry>,
): ReadonlySet<string> {
  const visible = new Set<string>();
  let batch: Array<Extract<TimelineEntry, { kind: "event" }>> = [];
  const flush = () => {
    if (batch.length === 0) return;
    if (batch.some((entry) => isLiveSubagentTurnItem(entry.projectedItem.item))) {
      for (const entry of batch) visible.add(entry.id);
    }
    batch = [];
  };
  for (const entry of entries) {
    if (entry.kind !== "event" || entry.projectedItem.item.type !== "subagent") {
      flush();
      continue;
    }
    const item = entry.projectedItem.item;
    const previous = batch.at(-1)?.projectedItem.item;
    if (
      previous !== undefined &&
      (previous.runId !== item.runId || previous.providerTurnId !== item.providerTurnId)
    ) {
      flush();
    }
    batch.push(entry);
  }
  flush();
  return visible;
}

function timelineEntryFoldRunId(entry: TimelineEntry, runlessKey: RunId | null): RunId | null {
  if (entry.kind === "work" && entry.entry.itemType === "system_notice") return null;
  if (entry.kind === "message" && entry.message.role === "assistant") {
    return entry.message.runId ?? runlessKey;
  }
  if (entry.kind === "work") {
    return entry.entry.runId ?? runlessKey;
  }
  if (
    entry.kind === "event" &&
    (timelineEntryIsPersistentResourceCard(entry) || entry.projectedItem.item.type === "subagent")
  ) {
    return entry.projectedItem.item.runId ?? runlessKey;
  }
  return null;
}

export function timelineEntryIsSteer(entry: TimelineEntry): boolean {
  return (
    entry.kind === "message" &&
    entry.message.role === "user" &&
    (entry.message.inputIntent === "steer" ||
      entry.message.inputIntent === "promoted_queued_to_steer")
  );
}

export function timelineEntryStartsResponse(entry: TimelineEntry): boolean {
  return (
    (entry.kind === "message" && entry.message.role === "user" && !timelineEntryIsSteer(entry)) ||
    (entry.kind === "work" && entry.entry.itemType === "notification")
  );
}

function timelineEntryEnd(entry: TimelineEntry): string {
  return entry.kind === "message" ? entry.message.updatedAt : entry.createdAt;
}

export function timelineEntryIsCompaction(entry: TimelineEntry): boolean {
  return entry.kind === "work" && entry.entry.sourceActivityKind === "context-compaction";
}

function responseBoundaryRunId(entry: TimelineEntry): RunId | null {
  if (entry.kind === "message") return entry.message.runId ?? null;
  if (entry.kind === "work") return entry.entry.runId ?? null;
  return null;
}

// Group promptless restarts under the initiating prompt until settled.
export function lastResponseBoundaryIndex(timelineEntries: ReadonlyArray<TimelineEntry>): number {
  return timelineEntries.findLastIndex(timelineEntryStartsResponse);
}

export function deriveActiveVisualResponseRunIds(input: {
  timelineEntries: ReadonlyArray<TimelineEntry>;
  unsettledRunId: RunId | null;
  isWorking: boolean;
}): ReadonlySet<RunId> {
  const runIds = new Set<RunId>();
  if (input.unsettledRunId === null) {
    return runIds;
  }

  runIds.add(input.unsettledRunId);
  if (!input.isWorking) {
    return runIds;
  }

  const latestResponseBoundaryIndex = lastResponseBoundaryIndex(input.timelineEntries);
  for (
    let index = latestResponseBoundaryIndex + 1;
    index < input.timelineEntries.length;
    index += 1
  ) {
    const runId = timelineEntryRunId(input.timelineEntries[index]!);
    if (runId !== null) {
      runIds.add(runId);
    }
  }
  return runIds;
}

export function timelineEntryFailedItem(entry: TimelineEntry) {
  const item =
    entry.kind === "event"
      ? entry.projectedItem.item
      : entry.kind === "work"
        ? entry.entry.projectedItem?.item
        : null;
  return item?.type === "error" && item.status === "failed" && item.parentItemId === null
    ? item
    : null;
}

export function failedTimelineRunIds(
  entries: ReadonlyArray<TimelineEntry>,
  latestRun: TimelineLatestRun | null,
) {
  const failed = new Set<RunId>();
  if (latestRun?.status === "failed") failed.add(latestRun.runId);
  for (const entry of entries) {
    const runId = timelineEntryFailedItem(entry)?.runId;
    if (runId) failed.add(runId);
  }
  return failed;
}

export function deriveTurnFolds(input: {
  timelineEntries: ReadonlyArray<TimelineEntry>;
  terminalAssistantMessageIds: ReadonlySet<string>;
  latestRun: TimelineLatestRun | null;
  unfoldedRunIds: ReadonlySet<RunId>;
  runlessWorkActive: boolean;
  liveSubagentEntryIds: ReadonlySet<string>;
  retainedAssistantReplyEntryIds: ReadonlySet<string>;
  resolve: MFunctionType;
}): ReadonlyMap<string, TurnFold> {
  const interruptedRunIds = new Set<RunId>();
  const promptIndexByRunId = new Map<RunId, number>();
  for (const [index, entry] of input.timelineEntries.entries()) {
    if (timelineEntryStartsResponse(entry)) {
      const runId = responseBoundaryRunId(entry);
      if (runId !== null && !promptIndexByRunId.has(runId)) promptIndexByRunId.set(runId, index);
    }
    if (
      entry.kind === "event" &&
      entry.projectedItem.item.runId !== null &&
      (entry.projectedItem.item.type === "run_interrupt_request" ||
        entry.projectedItem.item.type === "run_interrupt_result")
    ) {
      interruptedRunIds.add(entry.projectedItem.item.runId);
    }
  }

  interface TurnGroup {
    entries: Array<TimelineEntry>;
    terminalEntry: Extract<TimelineEntry, { kind: "message" }> | null;
    hasStreamingMessage: boolean;
    // Entry timestamps omit startup and can collapse steered turns to an instant.
    startBoundary: string | null;
    anchorEntryId: string;
    /**
     * Each steer after the turn has produced output starts a new segment at
     * this index into `entries`, so the steer sits between the work it split.
     */
    steerSegments: Array<{ startIndex: number; steer: TimelineEntry }>;
  }
  const groupsByRunId = new Map<RunId, TurnGroup>();
  const runlessFailedKeys = new Set<RunId>();

  // Key runless responses per prompt to prevent V2 runs unfolding imported turns.
  let runlessKey: RunId | null = null;
  let responseKey: RunId | null = null;
  let pendingBoundary: { createdAt: string; anchorEntryId: string } | null = null;
  for (const [index, entry] of input.timelineEntries.entries()) {
    if (timelineEntryStartsResponse(entry)) {
      const nextEntry = input.timelineEntries[index + 1];
      pendingBoundary = nextEntry
        ? { createdAt: entry.createdAt, anchorEntryId: nextEntry.id }
        : null;
      const boundaryRunId = responseBoundaryRunId(entry);
      runlessKey = boundaryRunId == null ? RunId.make(`runless:${entry.id}`) : null;
      responseKey = boundaryRunId ?? runlessKey;
      continue;
    }
    if (entry.kind === "message" && timelineEntryIsSteer(entry)) {
      const steerRunId = entry.message.runId ?? responseKey;
      const group = steerRunId === null ? undefined : groupsByRunId.get(steerRunId);
      if (group !== undefined && group.entries.length > 0) {
        // Back-to-back steers split once, after the last of them.
        if (group.steerSegments.at(-1)?.startIndex === group.entries.length) {
          group.steerSegments.pop();
        }
        group.steerSegments.push({ startIndex: group.entries.length, steer: entry });
      }
      continue;
    }
    const ownRunId = timelineEntryFoldRunId(entry, runlessKey);
    if (!ownRunId) {
      continue;
    }
    // Resumed subagents keep their position and fold with their original turn.
    const runId =
      responseKey !== null && (promptIndexByRunId.get(ownRunId) ?? -1) > index
        ? responseKey
        : ownRunId;
    if (runId === runlessKey && timelineEntryFailedItem(entry) !== null) {
      runlessFailedKeys.add(runId);
    }
    let group = groupsByRunId.get(runId);
    if (!group) {
      group = {
        entries: [],
        terminalEntry: null,
        hasStreamingMessage: false,
        startBoundary: pendingBoundary?.createdAt ?? null,
        anchorEntryId: pendingBoundary?.anchorEntryId ?? entry.id,
        steerSegments: [],
      };
      pendingBoundary = null;
      groupsByRunId.set(runId, group);
    }
    group.entries.push(entry);
    if (entry.kind === "message") {
      if (input.terminalAssistantMessageIds.has(entry.message.id)) {
        group.terminalEntry = entry;
      }
      if (entry.message.streaming) {
        group.hasStreamingMessage = true;
      }
    }
  }

  const foldsByAnchorEntryId = new Map<string, TurnFold>();
  for (const [runId, group] of groupsByRunId) {
    if (
      input.unfoldedRunIds.has(runId) ||
      interruptedRunIds.has(runId) ||
      runlessFailedKeys.has(runId) ||
      (input.runlessWorkActive && runId === runlessKey)
    ) {
      continue;
    }
    if (group.hasStreamingMessage) {
      continue;
    }
    const hiddenEntryIds = new Set<string>();
    const terminalEntryIndex = group.terminalEntry
      ? group.entries.findIndex((entry) => entry.id === group.terminalEntry?.id)
      : group.entries.length;
    // Folded dividers lose their place in the turn, so only the latest stays visible.
    const visibleCompactionId = group.entries.findLast(timelineEntryIsCompaction)?.id;
    for (const [index, entry] of group.entries.entries()) {
      if (entry.id === group.terminalEntry?.id) {
        continue;
      }
      const isCompaction = timelineEntryIsCompaction(entry);
      const isFoldableTrailingActivity =
        entry.kind === "work" &&
        entry.entry.toolLifecycleStatus !== "inProgress" &&
        !workEntryDisplayIndicatesToolFailure(entry.entry);
      if (!isCompaction && index > terminalEntryIndex && !isFoldableTrailingActivity) {
        continue;
      }
      if (
        entry.id === visibleCompactionId ||
        timelineEntryIsPersistentResourceCard(entry) ||
        input.liveSubagentEntryIds.has(entry.id) ||
        input.retainedAssistantReplyEntryIds.has(entry.id)
      ) {
        continue;
      }
      if (entry.kind === "work" && entry.entry.itemType === "notification") continue;
      if (entry.kind === "work" && entry.entry.questionAnswer !== undefined) continue;
      hiddenEntryIds.add(entry.id);
    }
    if (hiddenEntryIds.size === 0) {
      continue;
    }
    const hidesNonCompactionWork = (entries: ReadonlyArray<TimelineEntry>) =>
      entries.some((entry) => hiddenEntryIds.has(entry.id) && !timelineEntryIsCompaction(entry));
    if (!hidesNonCompactionWork(group.entries)) {
      continue;
    }

    const firstEntry = group.entries[0];
    const lastEntry = group.entries.at(-1);
    if (!firstEntry || !lastEntry) {
      continue;
    }

    const isLatestInterruptedTurn =
      input.latestRun?.runId === runId && input.latestRun.status === "interrupted";
    // A turn cut short by a steer leaves trailing work entries behind its
    // terminal message — take whichever ended last. A subagent card after it was
    // spawned by background work once the turn settled, so it adds no turn time.
    const lateSubagentIds = new Set(
      group.entries.flatMap((entry, index) =>
        index > terminalEntryIndex &&
        entry.kind === "event" &&
        entry.projectedItem.item.type === "subagent"
          ? [entry.id]
          : [],
      ),
    );
    const lastTurnEntry =
      group.entries.findLast((entry) => !lateSubagentIds.has(entry.id)) ?? lastEntry;
    const lastEntryEnd = timelineEntryEnd(lastTurnEntry);
    const elapsedMs =
      input.latestRun?.runId === runId && input.latestRun.startedAt && input.latestRun.completedAt
        ? computeElapsedMs(input.latestRun.startedAt, input.latestRun.completedAt)
        : computeElapsedMs(
            group.startBoundary ?? firstEntry.createdAt,
            maxIsoTimestamp(group.terminalEntry?.message.updatedAt ?? null, lastEntryEnd) ??
              lastEntryEnd,
          );
    const { resolve } = input;
    const duration = elapsedMs !== null ? formatDuration(elapsedMs, resolve) : null;
    const label = isLatestInterruptedTurn
      ? duration
        ? resolve(msg("You stopped after {duration}", { duration }))
        : resolve(msg("You stopped this response"))
      : duration
        ? resolve(msg("Worked for {duration}", { duration }))
        : resolve(msg("Worked"));

    // The top row keeps the whole turn's time, steers included. Work after a
    // steer folds under its own row below that steer, keyed apart from the run
    // so each segment opens on its own.
    const segmentHiddenIds = (entries: ReadonlyArray<TimelineEntry>) =>
      new Set(entries.flatMap((entry) => (hiddenEntryIds.has(entry.id) ? [entry.id] : [])));
    const firstSegment = group.entries.slice(0, group.steerSegments[0]?.startIndex);
    foldsByAnchorEntryId.set(group.anchorEntryId, {
      runId,
      anchorEntryId: group.anchorEntryId,
      createdAt: group.startBoundary ?? firstEntry.createdAt,
      hiddenEntryIds: segmentHiddenIds(firstSegment),
      entryIds: new Set(firstSegment.map((entry) => entry.id)),
      label,
    });
    for (const [index, { startIndex, steer }] of group.steerSegments.entries()) {
      const entries = group.entries.slice(startIndex, group.steerSegments[index + 1]?.startIndex);
      const anchorEntry = entries[0];
      if (anchorEntry === undefined || !hidesNonCompactionWork(entries)) continue;
      const segmentMs = computeElapsedMs(
        steer.createdAt,
        entries
          .filter((entry) => !lateSubagentIds.has(entry.id))
          .map(timelineEntryEnd)
          .reduce<string | null>(maxIsoTimestamp, null) ?? steer.createdAt,
      );
      foldsByAnchorEntryId.set(anchorEntry.id, {
        runId: RunId.make(`${runId}:steer:${steer.id}`),
        anchorEntryId: anchorEntry.id,
        createdAt: steer.createdAt,
        hiddenEntryIds: segmentHiddenIds(entries),
        entryIds: new Set(entries.map((entry) => entry.id)),
        label:
          segmentMs !== null
            ? resolve(
                msg("Continued for {duration}", { duration: formatDuration(segmentMs, resolve) }),
              )
            : resolve(msg("Continued")),
      });
    }
  }
  return foldsByAnchorEntryId;
}

type CollapsedTurnOptions = {
  latestRun: TimelineLatestRun | null;
  runningRunId: RunId | null;
  isWorking: boolean;
  runlessWorkActive: boolean;
  resolve?: MFunctionType;
};

export function collapsedTurnLayout(
  timelineEntries: readonly TimelineEntry[],
  input: CollapsedTurnOptions,
) {
  const unsettledRunId = deriveUnsettledRunId(input.latestRun, input.runningRunId);
  const failedRunIds = failedTimelineRunIds(timelineEntries, input.latestRun);
  const activeVisualResponseRunIds = deriveActiveVisualResponseRunIds({
    timelineEntries,
    unsettledRunId,
    isWorking: input.isWorking,
  });
  const terminalAssistantMessageIds = deriveTerminalAssistantMessageIds(timelineEntries);
  const folds = deriveTurnFolds({
    timelineEntries,
    terminalAssistantMessageIds,
    latestRun: input.latestRun,
    unfoldedRunIds: new Set([...activeVisualResponseRunIds, ...failedRunIds]),
    runlessWorkActive: input.isWorking && input.runlessWorkActive === true,
    liveSubagentEntryIds: liveSubagentCardEntryIds(timelineEntries),
    retainedAssistantReplyEntryIds: retainedAssistantReplyEntryIds(timelineEntries),
    resolve: input.resolve ?? mFallback,
  });
  return { folds, terminalAssistantMessageIds, activeVisualResponseRunIds };
}

export function collapsedTurnFolds(entries: readonly TimelineEntry[], input: CollapsedTurnOptions) {
  return collapsedTurnLayout(entries, input).folds;
}
