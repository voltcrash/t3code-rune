import type {
  ChatFileAttachment as ContractChatFileAttachment,
  ChatImageAttachment as ContractChatImageAttachment,
  ChatUnknownAttachment as ContractChatUnknownAttachment,
  MessageId,
  OrchestrationV2Actor,
  OrchestrationV2CreationSource,
  OrchestrationV2UserMessageInputIntent,
  OrchestrationV2LinkedSender,
  ScheduledTaskId,
  PlanId,
  OrchestrationV2ExecutionNode,
  OrchestrationV2PlanArtifact,
  OrchestrationV2ProjectedTurnItem,
  OrchestrationV2RunAttempt,
  OrchestrationV2TurnItem,
  RunId,
  ToolActivitySurface,
  ToolActivityIcon,
  ToolActivitySource,
  ThreadId,
  TurnItemId,
} from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import { msg, mFallback } from "gt-i18n";
import type { MFunctionType } from "gt-i18n/types";
import { extractToolActivityPresentation } from "./toolPresentation.ts";
import { classifyToolActivity, collectToolFilePaths } from "@t3tools/shared/toolActivity";
import { formatReadToolLabel, formatSearchToolLabel } from "@t3tools/shared/toolActivityCopy";
import type { HtmlRenderReference } from "@t3tools/shared/htmlRender";
import { turnItemDetailRevision } from "./itemDetail.ts";
import type { McpAppReference } from "@t3tools/shared/mcpApp";
import {
  htmlRenderFromToolItem,
  mcpAppFromToolItem,
  sentMessageFromToolItem,
} from "@t3tools/shared/toolOutput";
import { contextCompactionLabel, taskListLabel } from "./presentation.ts";
import { turnItemIsWorkspacePreparation } from "../state/turnItemPresentation.ts";

export interface ChatImageAttachment extends ContractChatImageAttachment {
  readonly previewUrl?: string;
}

export interface ChatFileAttachment extends ContractChatFileAttachment {
  readonly previewUrl?: string;
  readonly downloadable?: boolean;
  readonly htmlRender?: boolean;
}

// Render unknown attachment contracts as inert rows so newer servers cannot crash older clients.
export type ChatUnknownAttachment = ContractChatUnknownAttachment;

export type ChatAttachment = ChatImageAttachment | ChatFileAttachment | ChatUnknownAttachment;

export interface ChatMessage {
  readonly context?: import("@t3tools/contracts").OrchestrationMessageContext | undefined;
  readonly id: MessageId;
  readonly role: "user" | "assistant" | "system";
  readonly text: string;
  readonly attachments?: ReadonlyArray<ChatAttachment> | undefined;
  readonly runId: RunId | null;
  readonly streaming: boolean;
  readonly createdBy?: OrchestrationV2Actor;
  readonly creationSource?: OrchestrationV2CreationSource;
  readonly scheduledTaskId?: ScheduledTaskId;
  readonly senderThreadId?: ThreadId;
  readonly linkedSender?: OrchestrationV2LinkedSender;
  readonly idleCompaction?: import("@t3tools/contracts").OrchestrationV2IdleCompaction;
  readonly createdAt: string;
  readonly updatedAt: string;
  readonly inputIntent?: OrchestrationV2UserMessageInputIntent | undefined;
}

export interface ProposedPlan {
  readonly id: PlanId;
  readonly runId: RunId | null;
  readonly planMarkdown: string;
  readonly status: OrchestrationV2PlanArtifact["status"];
  readonly createdAt: string;
  readonly updatedAt: string;
}
export type WorkLogToolLifecycleStatus =
  | "idle"
  | "inProgress"
  | "completed"
  | "failed"
  | "declined"
  | "stopped";

export interface WorkLogEntry {
  readonly questionAnswer?: import("@t3tools/contracts").UserInputAttachmentAnswerPayload;
  readonly id: string;
  readonly createdAt: string;
  readonly runId?: RunId | null;
  readonly label: string;
  readonly detail?: string;
  readonly command?: string;
  readonly rawCommand?: string;
  readonly changedFiles?: ReadonlyArray<string>;
  readonly tone: "thinking" | "tool" | "info" | "error";
  readonly toolTitle?: string;
  readonly toolCallId?: string;
  readonly viewedImagePath?: string;
  readonly toolSurface?: ToolActivitySurface;
  readonly toolIcon?: ToolActivityIcon;
  readonly toolSource?: ToolActivitySource;
  readonly sourceActivityKind?: string;
  readonly taskId?: string;
  readonly agentRole?: string;
  readonly toolData?: unknown;
  readonly requestKind?: string;
  readonly itemType?: OrchestrationV2TurnItem["type"];
  readonly toolLifecycleStatus?: WorkLogToolLifecycleStatus;
  readonly structuredPayload?: OrchestrationV2TurnItem;
  readonly sourceItemType?: OrchestrationV2TurnItem["type"];
  readonly projectedItem?: OrchestrationV2ProjectedTurnItem;
}

export type TimelineAttempt = Pick<
  OrchestrationV2RunAttempt,
  "id" | "runId" | "attemptOrdinal" | "rootNodeId" | "status" | "completedAt"
>;

export type TimelineEntry = (
  | {
      readonly id: string;
      readonly kind: "message";
      readonly createdAt: string;
      readonly message: ChatMessage;
      readonly projectedItem?: OrchestrationV2ProjectedTurnItem;
    }
  | {
      readonly id: string;
      readonly kind: "proposed-plan";
      readonly createdAt: string;
      readonly proposedPlan: ProposedPlan;
    }
  | {
      readonly id: string;
      readonly kind: "html-render";
      readonly createdAt: string;
      readonly runId: RunId | null;
      readonly htmlRender: HtmlRenderReference;
    }
  | {
      readonly id: string;
      readonly kind: "sent-message";
      readonly createdAt: string;
      readonly runId: RunId | null;
      readonly text: string;
    }
  | {
      readonly id: string;
      readonly kind: "mcp-app";
      readonly createdAt: string;
      readonly runId: RunId | null;
      readonly sourceThreadId: ThreadId;
      readonly itemId: TurnItemId;
      readonly revision: string;
      readonly mcpApp: McpAppReference;
    }
  | {
      readonly id: string;
      readonly kind: "work";
      readonly createdAt: string;
      readonly entry: WorkLogEntry;
    }
  | {
      readonly id: string;
      readonly kind: "event";
      readonly createdAt: string;
      readonly projectedItem: OrchestrationV2ProjectedTurnItem;
    }
) & {
  readonly attempt?: TimelineAttempt;
};

const STANDALONE_V2_ITEM_TYPES = new Set<OrchestrationV2ProjectedTurnItem["item"]["type"]>([
  "fork",
  "handoff",
  "run_interrupt_request",
  "run_interrupt_result",
  "secret_request",
  "link_request",
  "subagent",
]);

const PERSISTENT_RESOURCE_V2_ITEM_TYPES = new Set<OrchestrationV2TurnItem["type"]>([
  "fork",
  "thread_created",
  // Still answerable after a steer supersedes the attempt that asked.
  "secret_request",
  "link_request",
]);

export function timelineEntryIsPersistentResourceCard(entry: TimelineEntry): boolean {
  return (
    entry.kind === "event" && PERSISTENT_RESOURCE_V2_ITEM_TYPES.has(entry.projectedItem.item.type)
  );
}

export function projectedItemCreatedAt(row: OrchestrationV2ProjectedTurnItem): string {
  return DateTime.formatIso(row.item.startedAt ?? row.item.updatedAt);
}

function projectedWorkEntryStatus(
  item: OrchestrationV2TurnItem,
): NonNullable<WorkLogEntry["toolLifecycleStatus"]> {
  switch (item.status) {
    case "pending":
    case "running":
    case "waiting":
      return "inProgress";
    case "completed":
      return "completed";
    case "idle":
      return "idle";
    case "failed":
      return "failed";
    case "cancelled":
    case "interrupted":
      return "stopped";
  }
}

function projectedWorkEntryTone(item: OrchestrationV2TurnItem): WorkLogEntry["tone"] {
  if (item.type === "error") return "info";
  if (item.type === "reasoning") return "thinking";
  switch (item.type) {
    case "command_execution":
    case "file_change":
    case "file_search":
    case "web_search":
    case "dynamic_tool":
    case "subagent":
    case "thread_created":
    case "user_input_request":
    case "approval_request":
      return "tool";
    default:
      return "info";
  }
}

export function providerErrorPresentation(
  item: Extract<OrchestrationV2TurnItem, { readonly type: "error" }>,
  resolve: MFunctionType = mFallback,
): { readonly label: string; readonly detail: string } {
  if (item.retry === undefined) {
    return {
      label:
        item.failure.class === "usage_limit"
          ? resolve(msg("Usage limit reached"))
          : item.title?.trim() || resolve(msg("Provider error")),
      detail: item.failure.message,
    };
  }
  const progress =
    item.retry.maxAttempts === null
      ? `${item.retry.attempt}`
      : `${item.retry.attempt}/${item.retry.maxAttempts}`;
  const counts = { progress, attempt: item.retry.attempt };
  const label =
    item.status === "running"
      ? resolve(msg("Retrying provider ({progress})", { progress }))
      : item.status === "completed"
        ? resolve(
            msg(
              "{attempt, plural, one {Provider recovered ({progress} retries)} other {Provider recovered ({progress} retries)}}",
            ),
            counts,
          )
        : item.status === "failed"
          ? item.failure.class === "usage_limit"
            ? resolve(
                msg(
                  "{attempt, plural, one {Usage limit reached after {progress} retries} other {Usage limit reached after {progress} retries}}",
                ),
                counts,
              )
            : resolve(
                msg(
                  "{attempt, plural, one {Provider error after {progress} retries} other {Provider error after {progress} retries}}",
                ),
                counts,
              )
          : resolve(msg("Provider retry stopped ({progress})", { progress }));
  const retryDelay =
    item.status === "running" && item.retry.retryDelayMs !== null && item.retry.retryDelayMs > 0
      ? item.retry.retryDelayMs < 1_000
        ? ` ${resolve(msg("Retrying in {delay}ms.", { delay: item.retry.retryDelayMs }))}`
        : ` ${resolve(
            msg("Retrying in {delay}s.", {
              delay: (item.retry.retryDelayMs / 1_000).toFixed(1).replace(/\.0$/u, ""),
            }),
          )}`
      : "";
  return {
    label,
    detail: `${item.failure.message}${retryDelay}`,
  };
}

function projectedWorkEntry(
  row: OrchestrationV2ProjectedTurnItem,
  resolve: MFunctionType,
): WorkLogEntry {
  const { item } = row;
  const title = item.title?.trim() || null;
  const common = {
    id: item.id,
    createdAt: projectedItemCreatedAt(row),
    runId: item.runId,
    tone: projectedWorkEntryTone(item),
    itemType: item.type,
    toolLifecycleStatus: projectedWorkEntryStatus(item),
    structuredPayload: item,
    projectedItem: row,
    ...extractToolActivityPresentation(item),
  } as const;

  switch (item.type) {
    case "thread_created":
      return {
        ...common,
        label: resolve(msg("Created thread")),
      };
    case "compaction":
      return {
        ...common,
        label: contextCompactionLabel(item, resolve),
        sourceActivityKind: "context-compaction",
        ...(item.summary ? { detail: item.summary } : {}),
      };
    case "reasoning":
      return {
        ...common,
        label: title ?? resolve(msg("Thinking", { $context: "status of an agent reasoning" })),
        ...(item.text ? { detail: item.text } : {}),
      };
    case "command_execution":
      return {
        ...common,
        label: title ?? resolve(msg("Ran command")),
        command: item.input,
        rawCommand: item.input,
        toolTitle: title ?? resolve(msg("Command", { $context: "noun, a shell command tool" })),
        toolData: item,
      };
    case "file_change": {
      return {
        ...common,
        label:
          title ??
          (item.changes !== undefined && item.changes.length > 1
            ? resolve(
                msg("{count, plural, one {Changed {count} file} other {Changed {count} files}}"),
                { count: item.changes.length },
              )
            : resolve(msg("Changed {fileName}", { fileName: item.fileName }))),
        changedFiles: item.changes?.length
          ? item.changes.map((change) => change.path)
          : [item.fileName],
        toolTitle: title ?? resolve(msg("File change", { $context: "noun, a tool title" })),
        toolData: item,
      };
    }
    case "file_search":
      return {
        ...common,
        label: title ?? formatSearchToolLabel(item, resolve) ?? resolve(msg("Searched files")),
        ...(item.pattern ? { detail: item.pattern } : {}),
        toolTitle: title ?? resolve(msg("File search", { $context: "noun, a tool title" })),
        toolData: item,
      };
    case "web_search":
      return {
        ...common,
        label: title ?? resolve(msg("Searched the web")),
        ...(item.patterns?.length ? { detail: item.patterns.join(", ") } : {}),
        toolTitle: title ?? resolve(msg("Web search", { $context: "noun, a tool title" })),
        toolData: item,
      };
    case "checkpoint":
      return {
        ...common,
        label: title ?? resolve(msg("Checkpoint captured")),
        changedFiles: item.files.map((file) => file.path),
        toolData: item,
      };
    case "system_notice":
      return {
        ...common,
        label: item.message,
        ...(item.level === "info" ? {} : { sourceActivityKind: "runtime.warning" }),
      };
    case "error": {
      const presentation = providerErrorPresentation(item, resolve);
      return {
        ...common,
        ...presentation,
        ...(item.failure.class === "usage_limit" && item.status !== "completed"
          ? { sourceActivityKind: "runtime.warning" }
          : item.retry === undefined
            ? { sourceActivityKind: "runtime.error" }
            : {}),
        toolData: item,
      };
    }
    case "dynamic_tool": {
      const classified = classifyToolActivity({
        itemType: "dynamic_tool_call",
        data: { toolName: item.toolName ?? undefined, input: item.input },
      });
      const [readPath] = collectToolFilePaths({ input: item.input });
      return {
        ...common,
        label:
          title ??
          (classified === "read"
            ? formatReadToolLabel(readPath ?? "", 0, resolve)
            : classified === "search"
              ? (formatSearchToolLabel({ input: item.input }, resolve) ??
                item.toolName ??
                resolve(msg("Tool call", { $context: "noun" })))
              : (item.toolName ?? resolve(msg("Tool call", { $context: "noun" })))),
        toolTitle:
          title ?? item.toolName ?? resolve(msg("Tool", { $context: "noun, a tool title" })),
        toolData: { input: item.input, output: item.output },
      };
    }
    case "todo_list": {
      // Each todo update is a quiet row where the agent wrote it; it opens to every step.
      return { ...common, label: taskListLabel(item.steps, resolve) };
    }
    case "approval_request":
      return {
        ...common,
        label: title ?? resolve(msg("Approval requested")),
        detail: item.prompt ?? item.requestKind,
        toolData: item,
      };
    case "user_input_request":
      return {
        ...common,
        label:
          title ??
          (item.questionAnswer
            ? resolve(msg("Answered questions"))
            : resolve(msg("Input requested"))),
        ...(item.questionAnswer ? { questionAnswer: item.questionAnswer } : {}),
        toolData: item,
      };
    default:
      return {
        ...common,
        label: title ?? item.type.replaceAll("_", " "),
        toolData: item,
      };
  }
}

/** Preserve visibleTurnItems' committed order; interleave client messages by timestamp. */
export interface TimelineEntriesInput {
  readonly visibleTurnItems: ReadonlyArray<OrchestrationV2ProjectedTurnItem>;
  readonly optimisticMessages: ReadonlyArray<ChatMessage>;
  readonly anchoredMessages?: ReadonlyArray<ChatMessage>;
  readonly attachmentUrlById?: ReadonlyMap<string, string>;
  readonly attempts?: ReadonlyArray<OrchestrationV2RunAttempt>;
  readonly nodes?: ReadonlyArray<OrchestrationV2ExecutionNode>;
  readonly plans?: ReadonlyArray<OrchestrationV2PlanArtifact>;
  readonly resolve?: MFunctionType;
}

export interface TimelineEntriesProjection {
  readonly input: TimelineEntriesInput;
  readonly entries: TimelineEntry[];
}

export function deriveTimelineEntriesFromVisibleTurnItems(
  input: TimelineEntriesInput,
): TimelineEntry[] {
  const committedMessageIds = new Set<string>();
  const entries: TimelineEntry[] = [];
  const attemptByRootNodeId = new Map(
    (input.attempts ?? []).map((attempt) => [attempt.rootNodeId, attempt] as const),
  );
  const nodeById = new Map((input.nodes ?? []).map((node) => [node.id, node] as const));
  const planById = new Map((input.plans ?? []).map((plan) => [plan.id, plan] as const));

  const resolveAttempt = (item: OrchestrationV2TurnItem): TimelineAttempt | undefined => {
    if (item.nodeId === null || item.runId === null) return undefined;
    let nodeId: OrchestrationV2ExecutionNode["id"] | null = item.nodeId;
    const visited = new Set<OrchestrationV2ExecutionNode["id"]>();
    while (nodeId !== null && !visited.has(nodeId)) {
      visited.add(nodeId);
      const directAttempt = attemptByRootNodeId.get(nodeId);
      if (directAttempt?.runId === item.runId) return directAttempt;
      const node = nodeById.get(nodeId);
      if (node === undefined) return undefined;
      const rootAttempt = attemptByRootNodeId.get(node.rootNodeId);
      if (rootAttempt?.runId === item.runId) return rootAttempt;
      nodeId = node.parentNodeId;
    }
    return undefined;
  };

  // An answer that started a later run stays a normal message at that run's position.
  const foldedAnswerRunIdByMessageId = new Map<string, OrchestrationV2TurnItem["runId"]>(
    input.visibleTurnItems.flatMap(({ item }) =>
      item.type === "user_input_request" && item.questionAnswer
        ? [[`async-answer:${item.questionAnswer.requestId}`, item.runId] as const]
        : [],
    ),
  );
  for (const row of input.visibleTurnItems) {
    const { item } = row;
    if (turnItemIsWorkspacePreparation(item) && !item.deferredWork) continue;
    if (item.type === "checkpoint") continue;
    if (
      item.type === "user_message" &&
      item.inputIntent === "queued_turn" &&
      item.status === "pending"
    )
      continue;
    if (
      item.type === "user_message" &&
      foldedAnswerRunIdByMessageId.get(item.messageId) === item.runId
    )
      continue;
    const createdAt = projectedItemCreatedAt(row);
    const attempt = resolveAttempt(item);
    const attemptMetadata = attempt === undefined ? {} : { attempt };
    if (item.type === "notification") {
      entries.push({
        id: item.id,
        kind: "work",
        createdAt,
        entry: {
          id: item.id,
          createdAt,
          runId: item.runId,
          label: item.summary,
          tone: "info",
          itemType: item.type,
          structuredPayload: item,
          projectedItem: row,
        },
        ...attemptMetadata,
      });
      continue;
    }
    if (item.type === "user_message" || item.type === "assistant_message") {
      const message: ChatMessage = {
        id: item.messageId,
        role: item.type === "user_message" ? "user" : "assistant",
        text: item.text,
        ...(item.type === "user_message" && item.context ? { context: item.context } : {}),
        ...((item.attachments?.length ?? 0) > 0
          ? {
              attachments: (item.attachments ?? []).map((attachment) => {
                const previewUrl = input.attachmentUrlById?.get(attachment.id);
                return previewUrl ? { ...attachment, previewUrl } : attachment;
              }),
            }
          : {}),
        runId: item.runId,
        streaming: item.type === "assistant_message" && item.streaming,
        ...(item.type === "user_message"
          ? {
              createdBy: item.createdBy,
              creationSource: item.creationSource,
              ...(item.senderThreadId !== undefined ? { senderThreadId: item.senderThreadId } : {}),
              ...(item.linkedSender !== undefined ? { linkedSender: item.linkedSender } : {}),
              ...(item.idleCompaction !== undefined ? { idleCompaction: item.idleCompaction } : {}),
              ...(item.scheduledTaskId !== undefined
                ? { scheduledTaskId: item.scheduledTaskId }
                : {}),
            }
          : {}),
        createdAt,
        updatedAt: DateTime.formatIso(item.updatedAt),
        ...(item.type === "user_message" ? { inputIntent: item.inputIntent } : {}),
      };
      committedMessageIds.add(message.id);
      entries.push({
        id: message.id,
        kind: "message",
        createdAt,
        message,
        projectedItem: row,
        ...attemptMetadata,
      });
      continue;
    }

    if (item.type === "proposed_plan") {
      const plan = planById.get(item.planId);
      const proposedPlan = {
        id: item.planId,
        runId: item.runId,
        planMarkdown: item.markdown,
        status: plan?.kind === "proposed_plan" ? plan.status : ("active" as const),
        createdAt,
        updatedAt: DateTime.formatIso(item.updatedAt),
      };
      entries.push({
        id: item.id,
        kind: "proposed-plan",
        createdAt,
        proposedPlan,
        ...attemptMetadata,
      });
      continue;
    }

    const htmlRender =
      item.type === "dynamic_tool" && item.status === "completed"
        ? htmlRenderFromToolItem(item)
        : undefined;
    if (htmlRender !== undefined) {
      entries.push({
        id: item.id,
        kind: "html-render",
        createdAt,
        runId: item.runId,
        htmlRender,
        ...attemptMetadata,
      });
      continue;
    }

    const sentMessage =
      item.type === "dynamic_tool" && item.status === "completed"
        ? sentMessageFromToolItem(item)
        : undefined;
    if (sentMessage !== undefined) {
      entries.push({
        id: item.id,
        kind: "sent-message",
        createdAt,
        runId: item.runId,
        text: sentMessage,
        ...attemptMetadata,
      });
      continue;
    }

    if (STANDALONE_V2_ITEM_TYPES.has(item.type)) {
      entries.push({
        id: item.id,
        kind: "event",
        createdAt,
        projectedItem: row,
        ...attemptMetadata,
      });
      continue;
    }

    entries.push({
      id: item.id,
      kind: "work",
      createdAt,
      entry: projectedWorkEntry(row, input.resolve ?? mFallback),
      ...attemptMetadata,
    });

    const mcpApp = item.type === "dynamic_tool" ? mcpAppFromToolItem(item) : undefined;
    if (mcpApp !== undefined) {
      entries.push({
        id: `mcp-app:${item.id}`,
        kind: "mcp-app",
        createdAt,
        runId: item.runId,
        sourceThreadId: row.sourceThreadId,
        itemId: row.sourceItemId,
        revision: turnItemDetailRevision(item),
        mcpApp,
        ...attemptMetadata,
      });
    }
  }

  const retainedMessageIds = new Set([
    ...committedMessageIds,
    ...foldedAnswerRunIdByMessageId.keys(),
  ]);
  for (const message of input.anchoredMessages ?? []) {
    if (retainedMessageIds.has(message.id)) continue;
    retainedMessageIds.add(message.id);
    const entry: TimelineEntry = {
      id: message.id,
      kind: "message",
      createdAt: message.createdAt,
      message,
    };
    const insertionIndex = entries.findIndex(
      (candidate) => candidate.createdAt > message.createdAt,
    );
    if (insertionIndex === -1) {
      entries.push(entry);
    } else {
      entries.splice(insertionIndex, 0, entry);
    }
  }

  for (const message of input.optimisticMessages) {
    if (message.inputIntent !== "queued_turn" && !retainedMessageIds.has(message.id)) {
      retainedMessageIds.add(message.id);
      entries.push({
        id: message.id,
        kind: "message",
        createdAt: message.createdAt,
        message,
      });
    }
  }

  return entries;
}
