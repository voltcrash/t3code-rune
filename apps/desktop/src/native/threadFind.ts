import {
  ORCHESTRATION_V2_WS_METHODS,
  ThreadId,
  type OrchestrationV2SearchThreadResult,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function threadFindLabels() {
  return {
    title: m(msg("Find in thread")),
    next: m(msg("Next match")),
    previous: m(msg("Previous match")),
    close: m(msg("Close find")),
    searching: m(msg("Searching…")),
    failed: m(msg("Search failed")),
    retry: m(msg("Retry")),
  };
}

export const findInThread = Effect.fn("nativeDesktop.findInThread")(function* (
  session: Session,
  progressive: boolean,
  thread: string,
  request: {
    query: string;
    index: number;
    dark: boolean;
    start?: { entryId: string; occurrence: number };
  },
  connected: Effect.Success<ReturnType<typeof openEnvironmentRpc>> | undefined,
  emit: (result: OrchestrationV2SearchThreadResult) => void,
) {
  const rpc = connected ?? (yield* openEnvironmentRpc(session));
  const input = {
    threadId: ThreadId.make(thread),
    query: request.query,
    ...(request.start ? { start: request.start } : { index: request.index }),
    theme: request.dark ? ("dark" as const) : ("light" as const),
  };
  if (progressive) {
    yield* rpc[ORCHESTRATION_V2_WS_METHODS.searchThreadStream](input).pipe(
      Stream.runForEach((result) => Effect.sync(() => emit(result))),
    );
  } else {
    emit(yield* rpc[ORCHESTRATION_V2_WS_METHODS.searchThread](input));
  }
});
