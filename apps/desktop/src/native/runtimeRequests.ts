// @effect-diagnostics nodeBuiltinImport:off -- The synchronous conversation reducer keys native controls by the content of the displayed provider request.
import * as NodeCrypto from "node:crypto";
import {
  buildPendingUserInputAnswers,
  type PendingUserInputDraftAnswer,
} from "@t3tools/client-runtime/state/pending-user-input";
import {
  type ThreadPendingUserInput,
  derivePendingThreadRequests,
  fullAccessOverrideExplanation,
} from "@t3tools/client-runtime/state/thread-requests";
import {
  CommandId,
  ORCHESTRATION_V2_WS_METHODS,
  type OrchestrationV2ThreadProjection,
  type ProviderApprovalDecision,
  type ProviderApprovalOption,
  RuntimeRequestId,
  ThreadId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";

const defaults: readonly ProviderApprovalOption[] = [
  { decision: "cancel", label: "Cancel" },
  { decision: "decline", label: "Decline" },
  { decision: "acceptForSession", label: "Always allow this session" },
  { decision: "accept", label: "Approve" },
];

const requestVersion = (request: unknown) =>
  NodeCrypto.createHash("sha256").update(JSON.stringify(request)).digest("hex");

export function pendingRequests(projection: OrchestrationV2ThreadProjection) {
  const pending = derivePendingThreadRequests(projection);
  const approvals = pending.approvals.map((approval) => {
    const present = projection.turnItems.some(
      (item) => item.type === "approval_request" && item.requestId === approval.requestId,
    );
    const options = present ? (approval.options ?? defaults) : [];
    return {
      id: approval.requestId,
      version: requestVersion([approval, present]),
      kind: approval.requestKind,
      detail: approval.detail,
      reason: approval.reason,
      app: approval.appName,
      explanation: approval.fullAccessOverride
        ? fullAccessOverrideExplanation(approval.fullAccessOverride)
        : undefined,
      live: approval.responseCapability === "live",
      present,
      options,
    };
  });
  const questions = pending.userInputs.map((prompt) => ({
    ...prompt,
    version: requestVersion(prompt),
  }));
  return { approvals, questions };
}

type Response = {
  thread: string;
  request: string;
  version: string;
  decision: ProviderApprovalDecision;
  operation: string;
};

export const respondToApproval = Effect.fn("nativeDesktop.respondToApproval")(function* (
  session: Parameters<typeof openEnvironmentRpc>[0],
  response: Response,
) {
  const rpc = yield* openEnvironmentRpc(session);
  return yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({
    type: "runtime-request.respond",
    commandId: CommandId.make(response.operation),
    threadId: ThreadId.make(response.thread),
    requestId: RuntimeRequestId.make(response.request),
    decision: response.decision,
  });
});

export const respondToQuestion = Effect.fn("nativeDesktop.respondToQuestion")(function* (
  session: Parameters<typeof openEnvironmentRpc>[0],
  prompt: ThreadPendingUserInput,
  response: {
    thread: string;
    operation: string;
    drafts: Record<string, PendingUserInputDraftAnswer> | null;
  },
) {
  const common = {
    commandId: CommandId.make(response.operation),
    threadId: ThreadId.make(response.thread),
    requestId: prompt.requestId,
  };
  const answers =
    response.drafts === null
      ? null
      : buildPendingUserInputAnswers(prompt.questions, response.drafts);
  if (
    response.drafts === null
      ? !prompt.dismissible
      : !answers || prompt.responseCapability === "not_resumable"
  )
    return false;
  const rpc = yield* openEnvironmentRpc(session);
  yield* rpc[ORCHESTRATION_V2_WS_METHODS.dispatchCommand](
    answers === null
      ? { type: "thread.user-input.dismiss", ...common }
      : { type: "runtime-request.respond", ...common, answers },
  );
  return true;
});
