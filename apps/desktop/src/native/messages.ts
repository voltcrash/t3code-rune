import {
  type ChatAttachment,
  CommandId,
  MessageId,
  RunId,
  ORCHESTRATION_V2_WS_METHODS,
  ThreadId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { committedAttachments, prepareAttachments, retryAttachments } from "./attachments.ts";
import { selectionForSend, type Handoff } from "./models.ts";
import type { Session } from "./projects.ts";

type PreparedMessage = {
  operation: string;
  attachments: readonly ChatAttachment[];
  attempted: boolean;
};
const pendingMessages = new WeakMap<Session, Map<string, PreparedMessage>>();

export const sendThreadMessage = Effect.fn("nativeDesktop.sendThreadMessage")(function* (
  session: Session,
  input: {
    thread: string;
    text: string;
    operation: string;
    files: readonly string[];
    model?: Handoff | null;
    permission?: { instance: string; mode: string } | null;
  },
) {
  const rpc = yield* openEnvironmentRpc(session);
  const pending = pendingMessages.get(session) ?? new Map<string, PreparedMessage>();
  const prior = pending.get(input.thread);
  if (prior?.operation !== input.operation) pending.delete(input.thread);
  if (
    (input.permission || input.model) &&
    prior?.operation === input.operation &&
    prior.attempted &&
    (yield* committedAttachments(session, input.thread, input.operation))
  ) {
    pending.delete(input.thread);
    return;
  }
  const attachments =
    prior?.operation === input.operation
      ? yield* retryAttachments(
          session,
          rpc,
          prior.attachments,
          input.files,
          input.thread,
          input.operation,
        )
      : yield* prepareAttachments(session, rpc, input.files, input.thread, input.operation);
  const prepared = {
    operation: input.operation,
    attachments,
    attempted: prior?.operation === input.operation && prior.attempted,
  };
  pending.set(input.thread, prepared);
  pendingMessages.set(session, pending);
  const modelSelection = yield* selectionForSend(session, rpc, input);
  prepared.attempted = true;
  const result = yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "message.dispatch",
    commandId: CommandId.make(input.operation),
    messageId: MessageId.make(input.operation),
    threadId: ThreadId.make(input.thread),
    createdBy: "user",
    creationSource: "web",
    text: input.text,
    attachments,
    ...(modelSelection ? { modelSelection } : {}),
    deliveryIntent: "auto",
    dispatchMode: { type: "start_immediately" },
  });
  pending.delete(input.thread);
  return result;
});

export const stopThreadRun = Effect.fn("nativeDesktop.stopThreadRun")(function* (
  session: Parameters<typeof openEnvironmentRpc>[0],
  input: { thread: string; run: string; operation: string },
) {
  const rpc = yield* openEnvironmentRpc(session);
  return yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "run.interrupt",
    commandId: CommandId.make(input.operation),
    threadId: ThreadId.make(input.thread),
    runId: RunId.make(input.run),
    holdQueue: true,
  });
});
