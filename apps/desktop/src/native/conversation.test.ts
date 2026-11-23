import {
  EventId,
  MessageId,
  RunId,
  TurnItemId,
  type OrchestrationV2TurnItem,
  type OrchestrationV2Run,
} from "@t3tools/contracts";
import { expect, it } from "vite-plus/test";
import {
  v2Now,
  v2Projection,
  v2ThreadId,
} from "../../../../packages/client-runtime/src/state/orchestrationV2TestFixtures.ts";
import { updateWindow } from "./conversation.ts";

it("keeps the previous final reply copyable when the next run receives its prompt", () => {
  const state: Parameters<typeof updateWindow>[0] = {
    layout: { folds: [], replies: [] },
    projection: v2Projection,
    projectRoot: undefined,
    activeRun: null,
    runlessWorkActive: false,
    requests: { approvals: [], questions: [] },
    sequence: 0,
    watermark: null,
    rows: [],
    indices: new Map(),
    cursor: null,
    earlier_pages: 0,
  };
  const event = { id: EventId.make("event"), threadId: v2ThreadId, occurredAt: v2Now };
  const run = (n: number, status: OrchestrationV2Run["status"]): OrchestrationV2Run => ({
    id: RunId.make("run-" + n),
    threadId: v2ThreadId,
    ordinal: n,
    providerInstanceId: v2Projection.thread.providerInstanceId,
    modelSelection: v2Projection.thread.modelSelection,
    providerThreadId: null,
    userMessageId: MessageId.make("user-" + n),
    rootNodeId: null,
    activeAttemptId: null,
    status,
    requestedAt: v2Now,
    startedAt: v2Now,
    completedAt: status === "completed" ? v2Now : null,
    checkpointId: null,
    contextHandoffId: null,
  });
  let ordinal = 0;
  let replies: readonly string[] = [];
  const apply = (input: Parameters<typeof updateWindow>[1]) =>
    updateWindow(state, input, (update) => {
      if (update.kind === "replies") replies = update.replies;
    });
  const message = (n: number, role: "user" | "assistant") => {
    const id = role + "-" + n;
    const base = {
      id: TurnItemId.make(id),
      threadId: v2ThreadId,
      runId: RunId.make("run-" + n),
      nodeId: null,
      providerThreadId: null,
      providerTurnId: null,
      nativeItemRef: null,
      parentItemId: null,
      ordinal: ++ordinal,
      status: "completed" as const,
      title: null,
      startedAt: v2Now,
      completedAt: v2Now,
      updatedAt: v2Now,
      messageId: MessageId.make(id),
      text: id,
    };
    const item: OrchestrationV2TurnItem =
      role === "assistant"
        ? { ...base, type: "assistant_message", streaming: false }
        : { ...base, type: "user_message", attachments: [], inputIntent: "prompt" };
    apply({ ...event, type: "turn-item.updated", payload: item });
  };
  apply({ ...event, type: "run.created", payload: run(1, "completed") });
  message(1, "user");
  message(1, "assistant");
  expect(replies).toEqual(["assistant-1"]);
  apply({ ...event, type: "run.created", payload: run(2, "running") });
  message(2, "user");
  expect(replies).toEqual(["assistant-1"]);
  message(2, "assistant");
  expect(replies).toEqual(["assistant-1"]);
  apply({ ...event, type: "run.updated", payload: run(2, "completed") });
  expect(replies).toEqual(["assistant-1", "assistant-2"]);
});
