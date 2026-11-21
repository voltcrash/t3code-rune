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
import { prepareAttachments, retryAttachments } from "./attachments.ts";
import type { Session } from "./projects.ts";

type PreparedAttachments = { operation: string; attachments: readonly ChatAttachment[] };
const pendingAttachments = new WeakMap<Session, Map<string, PreparedAttachments>>();

export const sendThreadMessage = Effect.fn("nativeDesktop.sendThreadMessage")(function* (
  session: Session,
  input: { thread: string; text: string; operation: string; files: readonly string[] },
) {
  const rpc = yield* openEnvironmentRpc(session);
  const pending = pendingAttachments.get(session) ?? new Map<string, PreparedAttachments>();
  const prior = pending.get(input.thread);
  if (prior?.operation !== input.operation) pending.delete(input.thread);
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
  if (attachments.length) {
    pending.set(input.thread, { operation: input.operation, attachments });
    pendingAttachments.set(session, pending);
  }
  const result = yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "message.dispatch",
    commandId: CommandId.make(input.operation),
    messageId: MessageId.make(input.operation),
    threadId: ThreadId.make(input.thread),
    createdBy: "user",
    creationSource: "web",
    text: input.text,
    attachments,
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
