import { makeEnvironmentHttpApiGroupClient } from "@t3tools/client-runtime/rpc";
import {
  type ChatAttachment,
  type ModelSelection,
  CommandId,
  MessageId,
  RunId,
  ORCHESTRATION_V2_WS_METHODS,
  ThreadId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import { committedAttachments, prepareAttachments, retryAttachments } from "./attachments.ts";
import { PermissionUnavailable, readThreadPermissions } from "./permissions.ts";
import { headersFor, type Session } from "./projects.ts";

type PreparedMessage = {
  operation: string;
  attachments: readonly ChatAttachment[];
  attempted: boolean;
};
const pendingMessages = new WeakMap<Session, Map<string, PreparedMessage>>();

const threadPermissionSnapshot = Effect.fn("nativeDesktop.threadPermissionSnapshot")(function* (
  session: Session,
  thread: string,
) {
  const api = yield* makeEnvironmentHttpApiGroupClient(session.httpBaseUrl, "orchestration");
  return yield* api.threadBoundedSnapshot({
    headers: headersFor(session),
    params: { threadId: ThreadId.make(thread) },
    query: { dedupeBodies: "1", compactTurnItems: "1", deferWork: "1" },
  });
});

export const sendThreadMessage = Effect.fn("nativeDesktop.sendThreadMessage")(function* (
  session: Session,
  input: {
    thread: string;
    text: string;
    operation: string;
    files: readonly string[];
    permission?: { instance: string; mode: string } | null;
  },
) {
  const rpc = yield* openEnvironmentRpc(session);
  const pending = pendingMessages.get(session) ?? new Map<string, PreparedMessage>();
  const prior = pending.get(input.thread);
  if (prior?.operation !== input.operation) pending.delete(input.thread);
  if (
    input.permission &&
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
  let modelSelection: ModelSelection | undefined;
  if (input.permission) {
    const { instance, mode } = input.permission;
    const options = yield* readThreadPermissions(rpc, instance);
    const snapshot = yield* threadPermissionSnapshot(session, input.thread);
    const thread = snapshot.projection.thread;
    const option = options.find((o) => o.mode === mode);
    if (
      thread.modelSelection.instanceId !== instance ||
      thread.archivedAt ||
      thread.deletedAt ||
      !option ||
      option.denied
    )
      return yield* new PermissionUnavailable({
        message: option?.denied ?? "Review permissions for the current provider before sending.",
      });
    modelSelection = thread.modelSelection;
    if (thread.runtimeMode !== option.mode) {
      yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
        type: "thread.runtime-mode.set",
        commandId: CommandId.make(input.operation + ":permissions:" + snapshot.snapshotSequence),
        threadId: thread.id,
        runtimeMode: option.mode,
      });
      // Existing servers persist the shared thread mode separately from dispatch, as in the web composer.
      const current = (yield* threadPermissionSnapshot(session, input.thread)).projection.thread;
      if (
        current.runtimeMode !== mode ||
        current.modelSelection.instanceId !== instance ||
        current.archivedAt ||
        current.deletedAt
      ) {
        return yield* new PermissionUnavailable({
          message:
            "This thread changed while saving permissions. Review the current selection before sending.",
        });
      }
      modelSelection = current.modelSelection;
    }
  }
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
