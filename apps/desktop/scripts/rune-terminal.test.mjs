import { WS_METHODS } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Stream from "effect/Stream";
import { expect, it, vi } from "vite-plus/test";
import { openEnvironmentRpc } from "../src/native/environmentRpc.ts";
import { openTerminal } from "./rune-terminal.mjs";

vi.mock("../src/native/environmentRpc.ts", () => ({ openEnvironmentRpc: vi.fn() }));

function drawer(rpc, operable) {
  openEnvironmentRpc.mockReturnValue(Effect.succeed(rpc));
  const controller = new AbortController();
  const watch = {
    owner: {
      canReadTerminal: true,
      canOperateTerminal: operable,
      http: Layer.empty,
      signal: controller.signal,
    },
    state: { projection: { thread: { worktreePath: null } }, projectRoot: "/fixture" },
    id: 1,
    thread: "thread",
    controller,
  };
  return {
    watch,
    open: (publish) => openTerminal(watch, { operation: 1, cols: 80, rows: 24 }, publish),
    [Symbol.dispose]: () => controller.abort(),
  };
}

it("observes the existing read-only session and keeps discovering sessions after output fails", async () => {
  const observed = Promise.withResolvers();
  const disconnected = Promise.withResolvers();
  const remaining = Promise.withResolvers();
  const initial = { threadId: "thread", terminalId: "term-2", label: "Original" };
  const next = { threadId: "thread", terminalId: "term-3", label: "Replacement" };
  using panel = drawer(
    {
      [WS_METHODS.subscribeTerminalMetadata]: () =>
        Stream.make({ type: "snapshot", terminals: [initial] }).pipe(
          Stream.concat(Stream.fromEffect(Effect.promise(() => disconnected.promise))),
          Stream.concat(Stream.make({ type: "upsert", terminal: next })),
          Stream.concat(Stream.never),
        ),
      [WS_METHODS.terminalObserve]: ({ terminalId }) => {
        observed.resolve(terminalId);
        return Stream.fail(new Error("Session closed"));
      },
    },
    false,
  );
  panel.open((event) => {
    if (event.status === "disconnected")
      disconnected.resolve({ type: "remove", threadId: "thread", terminalId: "term-2" });
    if (event.type === "terminalsessions" && event.rows[0]?.id === next.terminalId)
      remaining.resolve(event);
  });
  expect(await observed.promise).toBe("term-2");
  expect(await remaining.promise).toMatchObject({ active: "term-2", rows: [{ id: "term-3" }] });
});

it("reopens a remembered script terminal with its exit status and scrollback", async () => {
  const exited = {
    threadId: "thread",
    terminalId: "term-2",
    label: "Script",
    status: "exited",
    history: "saved-script-output",
    exitCode: 0,
  };
  const frame = Promise.withResolvers();
  const attach = vi.fn((input) =>
    Stream.make({
      type: "snapshot",
      snapshot: input.resume ? exited : { ...exited, status: "running", history: "" },
    }),
  );
  using panel = drawer(
    {
      [WS_METHODS.subscribeTerminalMetadata]: () =>
        Stream.make({ type: "snapshot", terminals: [exited] }).pipe(Stream.concat(Stream.never)),
      [WS_METHODS.terminalAttach]: attach,
    },
    true,
  );
  panel.watch.lastTerminal = exited.terminalId;
  panel.open((event) => {
    if (event.type === "terminal") frame.resolve(event);
  });
  const result = await frame.promise;
  expect(attach.mock.calls[0][0]).toMatchObject({ terminalId: "term-2", resume: true });
  expect(result.status).toBe("exited");
  expect(result.patches.flatMap(([, runs]) => runs.map((run) => run.text)).join("")).toContain(
    exited.history,
  );
});

it.each([
  ["term-1", ["term-1"]],
  ["term-2", []],
])("opens a usable default shell when reopening %s with sessions %j", async (lastTerminal, ids) => {
  const attached = Promise.withResolvers();
  using panel = drawer(
    {
      [WS_METHODS.subscribeTerminalMetadata]: () =>
        Stream.make({
          type: "snapshot",
          terminals: ids.map((terminalId) => ({ threadId: "thread", terminalId, label: "Shell" })),
        }).pipe(Stream.concat(Stream.never)),
      [WS_METHODS.terminalAttach]: (input) => {
        attached.resolve(input);
        return Stream.never;
      },
    },
    true,
  );
  panel.watch.lastTerminal = lastTerminal;
  panel.open(() => {});
  expect(await attached.promise).toMatchObject({ terminalId: "term-1", restartIfNotRunning: true });
});
