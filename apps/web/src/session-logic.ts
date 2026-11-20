import {
  deriveTimelineEntriesFromVisibleTurnItems as sharedEntries,
  providerErrorPresentation as sharedProviderError,
  projectedItemCreatedAt,
  type TimelineEntry,
  type TimelineEntriesProjection,
  type TimelineEntriesInput,
  type WorkLogEntry,
} from "@t3tools/client-runtime/work-log/timeline";
import { resolveThreadWorkingStartedAt } from "@t3tools/client-runtime/state/models";
import {
  type AssetResource,
  type OrchestrationV2Checkpoint,
  type OrchestrationV2ProjectedTurnItem,
  type OrchestrationV2TurnItem,
  type RunId,
} from "@t3tools/contracts";
import { mcpAppFromToolItem } from "@t3tools/shared/toolOutput";
import { workEntryIndicatesToolFailure } from "@t3tools/client-runtime/work-log/presentation";
import type { ThreadCheckpointSummary } from "@t3tools/client-runtime/state/thread-checkpoints";
import type {
  ThreadPendingApproval,
  ThreadPendingUserInput,
} from "@t3tools/client-runtime/state/thread-requests";
import type {
  ThreadStructureRuntime,
  ThreadRunSummary,
  ThreadRuntimeSummary,
} from "@t3tools/client-runtime/state/shell";
import { threadRuntimeHasInterruptibleRun } from "@t3tools/client-runtime/state/thread-execution";

import {
  isImageAttachment,
  type ChatAttachment,
  type ChatMessage,
  type SessionPhase,
  type TurnDiffSummary,
} from "./types";
import { m, type MessageResolver } from "./i18n/messages";
import * as DateTime from "effect/DateTime";
import * as Equal from "effect/Equal";
import { shallow } from "zustand/vanilla/shallow";

export {
  workEntryDisplayIndicatesToolFailure,
  workEntryIndicatesToolFailure,
} from "@t3tools/client-runtime/work-log/presentation";

export type {
  TimelineEntry,
  TimelineAttempt,
  TimelineEntriesInput,
  TimelineEntriesProjection,
  WorkLogEntry,
  WorkLogToolLifecycleStatus,
} from "@t3tools/client-runtime/work-log/timeline";
export { timelineEntryIsPersistentResourceCard } from "@t3tools/client-runtime/work-log/timeline";

export function providerErrorPresentation(
  item: Extract<OrchestrationV2TurnItem, { type: "error" }>,
  resolve: MessageResolver = m,
) {
  return sharedProviderError(item, resolve);
}

export function deriveTimelineEntriesFromVisibleTurnItems(
  input: TimelineEntriesInput,
): TimelineEntry[] {
  return sharedEntries({ ...input, resolve: input.resolve ?? m });
}
export type PendingApproval = ThreadPendingApproval;
export type PendingUserInput = ThreadPendingUserInput;

export function workLogEntryIsToolLike(entry: WorkLogEntry): boolean {
  return (
    entry.tone === "tool" ||
    entry.tone === "thinking" ||
    entry.tone === "error" ||
    entry.command !== undefined ||
    entry.requestKind !== undefined
  );
}

export function workEntrySignalsSevereFailure(entry: WorkLogEntry): boolean {
  return entry.itemType === "error";
}

export function workEntryIndicatesToolSuccess(entry: WorkLogEntry): boolean {
  if (
    !workLogEntryIsToolLike(entry) ||
    workEntryIndicatesToolFailure(entry) ||
    (entry.tone === "thinking" && entry.itemType !== "reasoning")
  ) {
    return false;
  }
  const status = entry.toolLifecycleStatus;
  return (
    status !== "failed" &&
    status !== "declined" &&
    status !== "inProgress" &&
    status !== "stopped" &&
    status !== "idle"
  );
}

export function workEntryIndicatesToolNeutralStatus(entry: WorkLogEntry): boolean {
  return (
    workLogEntryIsToolLike(entry) &&
    !workEntryIndicatesToolFailure(entry) &&
    !workEntryIndicatesToolSuccess(entry)
  );
}

export function isLatestRunSettled(
  latestRun: Pick<ThreadRunSummary, "runId" | "startedAt" | "completedAt" | "status"> | null,
  runtime: Pick<ThreadRuntimeSummary, "status" | "activeRunId"> | null,
): boolean {
  if (latestRun === null) return false;
  if (
    latestRun.status === "preparing" ||
    latestRun.status === "queued" ||
    latestRun.status === "starting" ||
    latestRun.status === "running" ||
    latestRun.status === "waiting"
  )
    return false;
  return runtime?.activeRunId !== latestRun.runId;
}

export function deriveActiveWorkStartedAt(
  latestRun: Pick<
    ThreadRunSummary,
    "runId" | "startedAt" | "requestedAt" | "completedAt" | "status"
  > | null,
  runtime: Pick<ThreadRuntimeSummary, "status" | "activeRunId" | "activityStartedAt"> | null,
  sendStartedAt: string | null,
): string | null {
  const startedAt = resolveThreadWorkingStartedAt({ latestRun, runtime });
  return startedAt ?? (runtime?.activeRunId == null ? sendStartedAt : null);
}

export function derivePendingApprovals(
  approvals: ReadonlyArray<ThreadPendingApproval>,
): ThreadPendingApproval[] {
  return [...approvals].toSorted((left, right) => left.createdAt.localeCompare(right.createdAt));
}

export function derivePendingUserInputs(
  inputs: ReadonlyArray<ThreadPendingUserInput>,
): ThreadPendingUserInput[] {
  return [...inputs].toSorted((left, right) => left.createdAt.localeCompare(right.createdAt));
}

type AttachmentResource = Extract<AssetResource, { readonly _tag: "attachment" }>;
const EMPTY_IMAGE_RESOURCES = Object.freeze<ReadonlyArray<AttachmentResource>>([]);

export function selectMessageImageResources(
  attachments: ChatMessage["attachments"],
): ReadonlyArray<AttachmentResource> {
  const attachmentIds = new Set<string>();
  for (const attachment of attachments ?? []) {
    if (!isImageAttachment(attachment)) continue;
    const previewUrl = attachment.previewUrl;
    if (previewUrl?.startsWith("blob:") || previewUrl?.startsWith("data:")) continue;
    attachmentIds.add(attachment.id);
  }
  return attachmentIds.size === 0
    ? EMPTY_IMAGE_RESOURCES
    : Array.from(attachmentIds, (attachmentId) => ({ _tag: "attachment", attachmentId }));
}

export function selectHandoffImageResources(
  messages: ReadonlyArray<Pick<ChatMessage, "id" | "role" | "attachments">> | undefined,
  handoffs: Readonly<Record<string, ReadonlyArray<string>>>,
): ReadonlyArray<AttachmentResource> {
  if (Object.keys(handoffs).length === 0) return EMPTY_IMAGE_RESOURCES;
  const attachmentIds = new Set<string>();
  for (const message of messages ?? []) {
    if (message.role !== "user" || !handoffs[message.id]?.length) continue;
    for (const attachment of message.attachments ?? []) {
      if (isImageAttachment(attachment)) attachmentIds.add(attachment.id);
    }
  }
  return attachmentIds.size === 0
    ? EMPTY_IMAGE_RESOURCES
    : Array.from(attachmentIds, (attachmentId) => ({ _tag: "attachment", attachmentId }));
}

export function createMessageAttachmentPreviewProjector() {
  const attachmentsBySource = new WeakMap<
    ReadonlyArray<ChatAttachment>,
    ReadonlyArray<ChatAttachment>
  >();
  const messagesBySource = new WeakMap<ChatMessage, ChatMessage>();
  return (
    message: ChatMessage,
    previewUrlFor: (attachment: ChatAttachment) => string | undefined,
  ): ChatMessage => {
    const source = message.attachments;
    if (!source || source.length === 0) return message;
    const previous = attachmentsBySource.get(source) ?? source;
    let changed: ChatAttachment[] | undefined;
    let hasOverrides = false;
    for (const [index, attachment] of source.entries()) {
      const previewUrl = previewUrlFor(attachment);
      const sourceUrl = "previewUrl" in attachment ? attachment.previewUrl : undefined;
      const previousAttachment = previous[index]!;
      const previousUrl =
        "previewUrl" in previousAttachment ? previousAttachment.previewUrl : undefined;
      const next =
        !previewUrl || previewUrl === sourceUrl
          ? attachment
          : previewUrl === previousUrl
            ? previousAttachment
            : { ...attachment, previewUrl };
      hasOverrides ||= next !== attachment;
      if (next !== previousAttachment) {
        changed ??= previous.slice();
        changed[index] = next;
      }
    }
    const attachments = hasOverrides ? (changed ?? previous) : source;
    attachmentsBySource.set(source, attachments);
    if (attachments === source) {
      messagesBySource.delete(message);
      return message;
    }
    const previousMessage = messagesBySource.get(message);
    if (previousMessage?.attachments === attachments) return previousMessage;
    const result = { ...message, attachments };
    messagesBySource.set(message, result);
    return result;
  };
}

export function isStreamingMessageTextUpdate(previous: ChatMessage, next: ChatMessage): boolean {
  if (
    previous.role !== "assistant" ||
    next.role !== "assistant" ||
    !previous.streaming ||
    !next.streaming
  ) {
    return false;
  }
  const { text: _previousText, updatedAt: _previousUpdatedAt, ...previousMetadata } = previous;
  const { text: _nextText, updatedAt: _nextUpdatedAt, ...nextMetadata } = next;
  return shallow(previousMetadata, nextMetadata);
}

function streamedTextMetadata(item: OrchestrationV2TurnItem) {
  if (item.type === "proposed_plan" && item.streaming) {
    const { markdown: _markdown, updatedAt: _updatedAt, ...metadata } = item;
    return metadata;
  }
  if ((item.type === "assistant_message" || item.type === "reasoning") && item.streaming) {
    const { text: _text, updatedAt: _updatedAt, ...metadata } = item;
    return metadata;
  }
  return null;
}

export function isStreamingTurnItemTextUpdate(
  previous: OrchestrationV2ProjectedTurnItem,
  next: OrchestrationV2ProjectedTurnItem,
): boolean {
  const { item: previousItem, ...previousSource } = previous;
  const { item: nextItem, ...nextSource } = next;
  const previousMetadata = streamedTextMetadata(previousItem);
  const nextMetadata = streamedTextMetadata(nextItem);
  if (
    previousMetadata === null ||
    nextMetadata === null ||
    !shallow(previousSource, nextSource) ||
    projectedItemCreatedAt(previous) !== projectedItemCreatedAt(next)
  ) {
    return false;
  }
  // Wire decoding recreates timestamps and attachments; compare only the changed item's metadata.
  return Equal.equals(previousMetadata, nextMetadata);
}

/**
 * A changed tool or work item maps to exactly one entry with the item's id.
 * Messages and question answers take part in cross-entry dedupe, and anchored
 * messages are placed by createdAt, so those changes rebuild.
 */
function canRederiveInPlace(
  input: TimelineEntriesInput,
  previousItem: OrchestrationV2ProjectedTurnItem,
  item: OrchestrationV2ProjectedTurnItem,
): boolean {
  return (
    item.item.id === previousItem.item.id &&
    item.item.type === previousItem.item.type &&
    item.item.type !== "user_message" &&
    item.item.type !== "assistant_message" &&
    item.item.type !== "user_input_request" &&
    // An app call projects two entries; in-place reuse replaces one.
    (previousItem.item.type !== "dynamic_tool" ||
      mcpAppFromToolItem(previousItem.item) === undefined) &&
    ((input.anchoredMessages?.length ?? 0) === 0 ||
      projectedItemCreatedAt(item) === projectedItemCreatedAt(previousItem))
  );
}

/** Entries read only each plan's status; streaming plans rebuild artifacts with every delta. */
function samePlanStatuses(
  next: TimelineEntriesInput["plans"],
  previous: TimelineEntriesInput["plans"],
): boolean {
  if (next === previous) return true;
  if (next === undefined || previous === undefined || next.length !== previous.length) return false;
  return next.every((plan, index) => {
    const before = previous[index]!;
    return plan.id === before.id && plan.kind === before.kind && plan.status === before.status;
  });
}

function reuseTimelineEntries(
  input: TimelineEntriesInput,
  previous: TimelineEntriesProjection,
): TimelineEntry[] | null {
  const before = previous.input;
  if (
    input.visibleTurnItems.length < before.visibleTurnItems.length ||
    !shallow(input.optimisticMessages, before.optimisticMessages) ||
    !shallow(input.anchoredMessages, before.anchoredMessages) ||
    !shallow(input.attachmentUrlById, before.attachmentUrlById) ||
    !shallow(input.attempts, before.attempts) ||
    !shallow(input.nodes, before.nodes) ||
    !samePlanStatuses(input.plans, before.plans) ||
    input.resolve !== before.resolve
  ) {
    return null;
  }
  const appended = input.visibleTurnItems.length > before.visibleTurnItems.length;
  // Keep full projection until committed items interleave and deduplicate anchored/optimistic messages.
  if (
    appended &&
    (input.optimisticMessages.length > 0 || (input.anchoredMessages?.length ?? 0) > 0)
  ) {
    return null;
  }
  // Answer rows can replace a message already present in the retained prefix.
  if (
    appended &&
    input.visibleTurnItems
      .slice(before.visibleTurnItems.length)
      .some(
        ({ item }) =>
          (item.type === "user_input_request" && item.questionAnswer !== undefined) ||
          (item.type === "user_message" && item.messageId.startsWith("async-answer:")),
      )
  )
    return null;
  const replacements = new Map<
    OrchestrationV2ProjectedTurnItem,
    OrchestrationV2ProjectedTurnItem
  >();
  const changedItems: OrchestrationV2ProjectedTurnItem[] = [];
  for (let index = 0; index < before.visibleTurnItems.length; index += 1) {
    const previousItem = before.visibleTurnItems[index]!;
    const item = input.visibleTurnItems[index]!;
    if (item === previousItem) continue;
    // Reasoning and plan text re-derive below; only assistant text patches the message.
    if (
      item.item.type === "assistant_message" &&
      isStreamingTurnItemTextUpdate(previousItem, item)
    ) {
      replacements.set(previousItem, item);
      continue;
    }
    if (!canRederiveInPlace(input, previousItem, item)) return null;
    changedItems.push(item);
  }
  // One batch keeps the attempt/node/plan lookups to a single build.
  const rederived =
    changedItems.length === 0
      ? []
      : deriveTimelineEntriesFromVisibleTurnItems({
          ...input,
          visibleTurnItems: changedItems,
          optimisticMessages: [],
          anchoredMessages: [],
        });
  if (
    rederived.length !== changedItems.length ||
    rederived.some((entry, index) => entry.id !== changedItems[index]!.item.id)
  ) {
    return null;
  }
  const rederivedEntries = new Map(rederived.map((entry) => [entry.id, entry]));
  if (replacements.size === 0 && rederivedEntries.size === 0 && !appended) {
    return previous.entries;
  }
  let rederivedCount = 0;
  const entries = previous.entries.map((entry): TimelineEntry => {
    const rederivedEntry = rederivedEntries.get(entry.id);
    if (rederivedEntry !== undefined) {
      rederivedCount += 1;
      return rederivedEntry;
    }
    const row =
      entry.kind === "message" && entry.projectedItem !== undefined
        ? replacements.get(entry.projectedItem)
        : undefined;
    if (entry.kind !== "message" || row?.item.type !== "assistant_message") return entry;
    return {
      ...entry,
      projectedItem: row,
      message: {
        ...entry.message,
        text: row.item.text,
        updatedAt: DateTime.formatIso(row.item.updatedAt),
      },
    };
  });
  if (rederivedCount !== rederivedEntries.size) return null;
  if (appended) {
    entries.push(
      ...deriveTimelineEntriesFromVisibleTurnItems({
        ...input,
        visibleTurnItems: input.visibleTurnItems.slice(before.visibleTurnItems.length),
      }),
    );
  }
  return entries;
}

export function deriveTimelineEntriesFromVisibleTurnItemsWithState(
  input: TimelineEntriesInput,
  previous: TimelineEntriesProjection | null = null,
): TimelineEntriesProjection {
  const reused = previous === null ? null : reuseTimelineEntries(input, previous);
  if (reused !== null) return { input, entries: reused };
  const entries = deriveTimelineEntriesFromVisibleTurnItems(input);
  if (previous === null || !shallow(input.attachmentUrlById, previous.input.attachmentUrlById)) {
    return { input, entries };
  }
  const previousMessages = new Map(
    previous.entries.flatMap((entry) =>
      entry.kind === "message" ? [[entry.id, entry] as const] : [],
    ),
  );
  return {
    input,
    entries: entries.map((entry) => {
      if (entry.kind !== "message" || entry.projectedItem === undefined) return entry;
      const before = previousMessages.get(entry.id);
      if (before?.projectedItem !== entry.projectedItem) return entry;
      return before.attempt === entry.attempt ? before : { ...entry, message: before.message };
    }),
  };
}

// Run ordinals keep increasing after a rewind; only visible local runs define predecessors.
export function deriveRevertTurnCountByUserMessageId(input: {
  readonly timelineEntries: ReadonlyArray<TimelineEntry>;
  readonly checkpoints: ReadonlyArray<ThreadCheckpointSummary>;
  readonly threadStartCheckpointAvailable: boolean;
  readonly availableRevertTurnCounts?: ReadonlySet<number> | undefined;
  readonly unsettledRunId?: RunId | null;
}): Map<ChatMessage["id"], number> {
  const turnCountByRunId = new Map<RunId, number>();
  for (const checkpoint of input.checkpoints) {
    if (isRewindableCheckpoint(checkpoint)) {
      turnCountByRunId.set(checkpoint.runId, checkpoint.checkpointTurnCount);
    }
  }
  const targetByRunId = new Map<RunId, number | undefined>();
  const byUserMessageId = new Map<ChatMessage["id"], number>();
  let previousRunId: RunId | null = null;
  for (const entry of input.timelineEntries) {
    // Inherited history belongs to the source thread, not our checkpoint scope.
    if (entry.kind !== "message" && entry.kind !== "work") continue;
    const projectedItem =
      entry.kind === "message" ? entry.projectedItem : entry.entry.projectedItem;
    if (projectedItem?.visibility === "inherited") continue;
    const runId =
      entry.kind === "message"
        ? entry.message.runId
        : entry.entry.itemType === "notification"
          ? entry.entry.runId
          : null;
    if (runId == null) continue;
    if (!targetByRunId.has(runId)) {
      targetByRunId.set(
        runId,
        previousRunId === null
          ? input.threadStartCheckpointAvailable
            ? 0
            : undefined
          : turnCountByRunId.get(previousRunId),
      );
      previousRunId = runId;
    }
    if (entry.kind !== "message") continue;
    const { message } = entry;
    if (message.role !== "user" || message.runId === input.unsettledRunId) continue;
    if (message.inputIntent !== "turn_start" && message.inputIntent !== "queued_turn") continue;
    const target = targetByRunId.get(runId);
    if (
      target !== undefined &&
      (input.availableRevertTurnCounts === undefined || input.availableRevertTurnCounts.has(target))
    )
      byUserMessageId.set(message.id, target);
  }
  return byUserMessageId;
}

// A missing snapshot (no git, or a side chat) still pins a conversation position; only
// restoring files needs a ready one.
function isRewindableCheckpoint(checkpoint: Pick<OrchestrationV2Checkpoint, "status">) {
  return checkpoint.status === "ready" || checkpoint.status === "missing";
}

export function findRevertCheckpoint(
  checkpoints: ReadonlyArray<OrchestrationV2Checkpoint>,
  turnCount: number,
): OrchestrationV2Checkpoint | undefined {
  return checkpoints.findLast(
    (checkpoint) =>
      isRewindableCheckpoint(checkpoint) &&
      (turnCount === 0
        ? checkpoint.ordinalWithinScope === 0 && checkpoint.appRunOrdinal === null
        : checkpoint.appRunOrdinal === turnCount),
  );
}

export function derivePhase(runtime: ThreadStructureRuntime | null): SessionPhase {
  if (runtime === null) return "disconnected";
  if (
    runtime.status === "preparing" ||
    runtime.status === "starting" ||
    runtime.status === "queued"
  )
    return "connecting";
  if (runtime.status === "running" || runtime.status === "waiting") return "running";
  return "ready";
}

/** Preparing and starting runs can be interrupted; Stop need not wait for running. */
export function deriveCanInterruptRunningThread(
  hasActiveThread: boolean,
  runtime: ThreadStructureRuntime | null,
): boolean {
  return (
    hasActiveThread &&
    (derivePhase(runtime) === "running" || threadRuntimeHasInterruptibleRun(runtime))
  );
}

export type { TurnDiffSummary };
