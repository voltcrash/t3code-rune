import {
  CommandId,
  MessageId,
  RunId,
  ORCHESTRATION_V2_WS_METHODS,
  ThreadId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";

export const sendThreadMessage = Effect.fn("nativeDesktop.sendThreadMessage")(function* (
  session: Parameters<typeof openEnvironmentRpc>[0],
  input: { thread: string; text: string; operation: string },
) {
  const rpc = yield* openEnvironmentRpc(session);
  return yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "message.dispatch",
    commandId: CommandId.make(input.operation),
    messageId: MessageId.make(input.operation),
    threadId: ThreadId.make(input.thread),
    createdBy: "user",
    creationSource: "web",
    text: input.text,
    attachments: [],
    deliveryIntent: "auto",
    dispatchMode: { type: "start_immediately" },
  });
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
