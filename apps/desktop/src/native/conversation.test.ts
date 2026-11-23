import {
  EventId,
  MessageId,
  RunId,
  TurnItemId,
  type OrchestrationV2TurnItem,
  type OrchestrationV2Run,
} from "@t3tools/contracts";
import { searchableMessageSegments } from "@t3tools/shared/threadFindText";
import { expect, it } from "vite-plus/test";
import { v2Now, v2Projection, v2ThreadId } from "@t3tools/client-runtime/testing/thread-projection";
import { updateWindow } from "./conversation.ts";

it("keeps the previous final reply copyable when the next run receives its prompt", () => {
  const state: Parameters<typeof updateWindow>[0] = {
    layout: { folds: [], replies: [] },
    projection: v2Projection,
    projectRoot: "/repo",
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
  const message = (n: number, role: "user" | "assistant", body?: string) => {
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
      text: body ?? id,
    };
    const item: OrchestrationV2TurnItem =
      role === "assistant"
        ? { ...base, type: "assistant_message", streaming: false }
        : {
            ...base,
            type: "user_message",
            attachments: [],
            inputIntent: "turn_start",
            createdBy: "user",
            creationSource: "server",
          };
    apply({ ...event, type: "turn-item.updated", payload: item });
  };
  apply({ ...event, type: "run.created", payload: run(1, "completed") });
  message(1, "user");
  message(
    1,
    "assistant",
    'See :codex-file-citation{path="src/main.rs" line_range_start="12"}, then L12. ![Diagram](docs/main.rs)',
  );
  const linked = state.rows.at(-1)!;
  expect(linked.body).toBe(
    "See [main.rs · repo/src · L12](<rune-file:4>), then L12. [main.rs · repo/docs](<rune-file:78>)",
  );
  expect(linked.file_links?.["rune-file:4"]).toEqual({
    href: "src/main.rs#L12",
    copy: "[main.rs](src/main.rs#L12)",
  });
  expect(linked.copy_body).toBe(
    "See [main.rs](<src/main.rs#L12>), then L12. ![Diagram](docs/main.rs)",
  );
  for (const example of ["```md\n[example](docs/main.rs)\n```", "[API [draft]](docs/main.rs)"]) {
    const text = ':codex-file-citation{path="src/main.rs" line_range_start="12"}\n\n' + example;
    message(1, "assistant", text);
    const row = state.rows.at(-1)!;
    const indexed = searchableMessageSegments(
      { role: "assistant", streaming: false, text },
      "/repo",
    )!;
    expect(row.body.split("\n\n")[0]).toBe(`[${indexed[0]}](<rune-file:0>)`);
    if (example.startsWith("[")) {
      expect(indexed[1]).toBe("API [draft] main.rs");
      expect(row.body.split("\n\n")[1]).toBe("API \\[draft\\] [main.rs](<rune-file:64>)");
      expect(row.file_links?.["rune-file:64"]).toEqual({
        href: "docs/main.rs",
        copy: example,
        prefix: "API [draft] ",
      });
    } else expect(row.body.split("\n\n")[1]).toBe(example);
  }
  message(1, "assistant", "[# Read **this**](docs/main.rs)");
  expect(state.rows.at(-1)?.body).toBe("\\# Read **this** [main.rs](<rune-file:0>)");
  expect(replies).toEqual(["assistant-1"]);
  apply({ ...event, type: "run.created", payload: run(2, "running") });
  message(2, "user");
  expect(replies).toEqual(["assistant-1"]);
  const literal =
    '::artifact-template{skill_name="artifact-template-hello-world" skill_directory="/skills/hello" display_name="Hello World" artifact_kind="document"}';
  message(2, "assistant", `:codex-followup[${literal}]{prompt="Continue"}`);
  const response = state.rows.at(-1);
  expect(response?.copy_body ?? response?.body).toBe(literal);
  expect(replies).toEqual(["assistant-1"]);
  apply({ ...event, type: "run.updated", payload: run(2, "completed") });
  expect(replies).toEqual(["assistant-1", "assistant-2"]);
});
