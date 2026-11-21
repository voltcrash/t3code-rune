import { ThreadId, TurnItemId, type OrchestrationV2ProjectedTurnItem } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import { expect, it } from "vite-plus/test";

import { toolRow } from "./toolDetail.ts";

it("renders a delivered agent message inline and keeps unsuccessful calls inspectable", () => {
  const threadId = ThreadId.make("thread");
  const id = TurnItemId.make("sent");
  const item = {
    type: "dynamic_tool",
    id,
    threadId,
    runId: null,
    nodeId: null,
    parentItemId: null,
    providerThreadId: null,
    providerTurnId: null,
    nativeItemRef: null,
    ordinal: 1,
    status: "completed",
    startedAt: null,
    completedAt: null,
    updatedAt: DateTime.makeUnsafe("2026-10-01T00:00:00Z"),
    title: null,
    toolName: "mcp__t3-code__send_to_user",
    input: {},
    output: { sentMessage: "## Draft\n\nReady for review." },
  } as const satisfies OrchestrationV2ProjectedTurnItem["item"];
  const row = {
    position: 1,
    sourceItemId: id,
    sourceThreadId: threadId,
    visibility: "inherited",
  } as const;
  const render = (item: OrchestrationV2ProjectedTurnItem["item"]) =>
    toolRow("entry", { ...row, item });
  for (const toolName of [item.toolName, "execute"]) {
    expect(render({ ...item, toolName })).toEqual({
      key: "entry",
      role: "sent-message",
      body: item.output.sentMessage,
      attachments: [],
    });
  }
  expect(render({ ...item, status: "running" }).role).toBe("tool");
  expect(render({ ...item, status: "failed" }).role).toBe("tool");
  expect(render({ ...item, output: { ...item.output, isError: true } }).role).toBe("tool");
  expect(render({ ...item, toolName: "unrelated" }).role).toBe("tool");
});
