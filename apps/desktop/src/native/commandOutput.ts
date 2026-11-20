import {
  applyCommandOutputFrame,
  EMPTY_COMMAND_OUTPUT,
} from "@t3tools/client-runtime/state/command-output";
import { ORCHESTRATION_V2_WS_METHODS, type ThreadId, type TurnItemId } from "@t3tools/contracts";
import { normalizeTerminalOutput, terminalOutputPlainText } from "@t3tools/shared/terminalOutput";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export const readCommandOutput = Effect.fn("nativeDesktop.readCommandOutput")(function* (
  session: Session,
  input: { threadId: ThreadId; itemId: TurnItemId },
  emit: (text: string) => void,
) {
  const rpc = yield* openEnvironmentRpc(session);
  if (!session.streamsCommandOutput) {
    const { item } = yield* rpc[ORCHESTRATION_V2_WS_METHODS.getTurnItem](input);
    if (item?.type !== "command_execution") return emit("Command unavailable.");
    const output = normalizeTerminalOutput(item.output ?? "");
    return emit(
      (output.truncated ? "Earlier output not shown\n" : "") +
        (terminalOutputPlainText(output.text) || "No output."),
    );
  }
  let view = EMPTY_COMMAND_OUTPUT;
  yield* rpc[ORCHESTRATION_V2_WS_METHODS.subscribeCommandOutput](input).pipe(
    Stream.takeUntil((frame) => !frame.running),
    Stream.groupedWithin(32, "50 millis"),
    Stream.runForEach((frames) =>
      Effect.sync(() => {
        for (const frame of frames) view = applyCommandOutputFrame(view, frame);
        const text = terminalOutputPlainText(view.output.text);
        emit(
          (view.output.truncated ? "Earlier output not shown\n" : "") +
            (text || (view.running ? "" : "No output.")),
        );
      }),
    ),
  );
});
