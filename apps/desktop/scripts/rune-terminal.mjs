import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import { projectScriptRuntimeEnv } from "@t3tools/shared/projectScripts";
import { GhosttyTerminalCore } from "@t3tools/client-runtime/terminal/ghostty/core";
import { GhosttyRuntime } from "@t3tools/client-runtime/terminal/ghostty/runtime";
import { TERMINAL_PALETTES } from "@t3tools/client-runtime/terminal/ghostty/palette";
import {
  DEFAULT_MAX_TERMINAL_BUFFER_BYTES,
  DEFAULT_TERMINAL_ID,
  WS_METHODS,
  chunkTerminalWrite,
} from "@t3tools/contracts";
import {
  nextTerminalAttachSeedState,
  applyTerminalAttachStreamEvent,
  INITIAL_TERMINAL_OUTPUT_CURSOR,
  readTerminalOutputUpdate,
} from "@t3tools/client-runtime/state/terminal-session";
import { openEnvironmentRpc } from "../src/native/environmentRpc.ts";

let loading;
async function loadRuntime() {
  const names = ["ghostty-vt.wasm", "ghostty-write-pty.wasm"];
  const mode = process.argv[2];
  const assets = "src/terminal/ghostty/vendor";
  let paths;
  if (mode === "--packaged" || mode === "--source-build") {
    const client = NodePath.resolve(
      import.meta.dirname,
      mode === "--packaged" ? "server/dist/client" : "../../../server/dist/client",
    );
    const manifest = JSON.parse(
      await NodeFSP.readFile(NodePath.join(client, ".vite/manifest.json"), "utf8"),
    );
    paths = names.map((name) => NodePath.join(client, manifest[assets + "/" + name].file));
  } else {
    paths = names.map((name) => NodePath.resolve(import.meta.dirname, "../../web", assets, name));
  }
  const [main, trampoline] = await Promise.all(paths.map((path) => NodeFSP.readFile(path)));
  return GhosttyRuntime.load(main, trampoline, (message) =>
    console.error("[libghostty-vt]", message),
  );
}

const rgb = ({ r, g, b }) => (r << 16) | (g << 8) | b;
function theme(dark) {
  return {
    foreground: dark ? { r: 245, g: 245, b: 245 } : { r: 24, g: 24, b: 27 },
    background: dark ? { r: 0, g: 0, b: 0 } : { r: 253, g: 253, b: 253 },
    cursor: dark ? { r: 180, g: 203, b: 255 } : { r: 38, g: 56, b: 78 },
    palette: TERMINAL_PALETTES[dark ? "dark" : "light"],
  };
}

function runs(row, background) {
  const result = [];
  for (const [x, cell] of row.cells.entries()) {
    if (cell.wide >= 2) continue;
    const value = cell.invisible ? " " : cell.text || " ";
    const fg = rgb(cell.foreground),
      bg = rgb(cell.background);
    if (
      value === " " &&
      bg === background &&
      !cell.underline &&
      !cell.strikethrough &&
      !cell.overline
    )
      continue;
    const wide = cell.wide === 1 ? 2 : 1;
    const style =
      +cell.bold |
      (+cell.italic << 1) |
      (+cell.underline << 2) |
      (+cell.strikethrough << 3) |
      (+cell.overline << 4);
    const last = result.at(-1);
    if (
      last &&
      last.x + last.width === x &&
      last.fg === fg &&
      last.bg === bg &&
      last.style === style &&
      wide === 1
    ) {
      last.text += value;
      last.width++;
    } else result.push({ x, width: wide, text: value, fg, bg, style });
  }
  return result;
}

export function openTerminal(watch, request, write) {
  const { owner, state } = watch;
  const operable = owner.canOperateTerminal;
  if (!operable && !owner.canReadTerminal) return;
  watch.terminal?.close();
  const controller = new AbortController();
  const signal = AbortSignal.any([controller.signal, watch.controller.signal, owner.signal]);
  const reply = (body) => {
    if (!signal.aborted)
      write({ type: "terminal", watch: watch.id, operation: request.operation, ...body });
  };
  let core,
    rpc,
    timer,
    waiting = false,
    dirty = false,
    force = true;
  let cols = request.cols,
    rows = request.rows,
    dark = request.dark;
  let status = "connecting",
    error = null;
  const id = { threadId: watch.thread, terminalId: DEFAULT_TERMINAL_ID };
  const run = (effect) => Effect.runPromise(effect.pipe(Effect.provide(owner.http)), { signal });
  let sharedGrid = null;
  let reportedGrid;
  let resizing = false;
  function reportGrid() {
    if (signal.aborted || !operable || !rpc || status !== "running" || resizing) return;
    if (reportedGrid?.cols === cols && reportedGrid.rows === rows) return;
    reportedGrid = { cols, rows };
    resizing = true;
    void run(rpc[WS_METHODS.terminalResize]({ ...id, cols, rows }))
      .catch(() => {
        if (!signal.aborted) {
          error = "Couldn’t resize the terminal.";
          invalidate();
        }
      })
      .finally(() => {
        resizing = false;
        reportGrid();
      });
  }
  let writes = Promise.resolve();
  function enqueue(effect) {
    writes = writes
      .then(() => (signal.aborted ? undefined : run(effect)))
      .catch(() => {
        if (!signal.aborted) {
          error = "Terminal input could not be delivered.";
          invalidate();
        }
      });
  }
  function send(data, isReply = false) {
    if (!rpc || !operable || !data || status !== "running") return;
    for (const chunk of chunkTerminalWrite(data))
      enqueue(
        rpc[WS_METHODS.terminalWrite]({ ...id, data: chunk, ...(isReply ? { reply: true } : {}) }),
      );
  }
  function invalidate() {
    dirty = true;
    if (timer || waiting || !core || signal.aborted) return;
    timer = setTimeout(() => {
      timer = undefined;
      if (signal.aborted) return;
      const frame = core.snapshot();
      const bg = rgb(frame.background);
      const patches = [];
      for (let y = 0; y < frame.rows; y++) {
        if (force || frame.dirtyRows.has(y)) patches.push([y, runs(frame.rowData[y], bg)]);
      }
      waiting = true;
      dirty = false;
      force = false;
      reply({
        status,
        error,
        rows: frame.rows,
        background: bg,
        cursor: frame.cursorVisible
          ? [frame.cursorX, frame.cursorY, rgb(frame.cursor), frame.cursorStyle]
          : null,
        patches,
      });
    }, 16);
  }
  const panel = {
    operation: request.operation,
    close: () => controller.abort(),
    input(event) {
      if (event.action === "handled") {
        waiting = false;
        if (dirty) invalidate();
        return;
      }
      if (event.action === "resize") {
        const changed = cols !== event.cols || rows !== event.rows;
        cols = event.cols;
        rows = event.rows;
        if (dark !== event.dark) {
          dark = event.dark;
          core?.setTheme(theme(dark));
          force = true;
          invalidate();
        }
        if (changed && core && operable && rpc && status === "running") {
          if (!sharedGrid) {
            core.resize(cols, rows, 7.23, 16);
            force = true;
            invalidate();
          }
          reportGrid();
        }
      }
      if (!core) return;
      if (event.action === "scroll") {
        core.scroll(event.lines);
        invalidate();
      }
      if (event.action === "key" && operable) {
        const key = { ...event, getModifierState: () => false, isComposing: false };
        const data =
          event.printable && event.key.length > 1 && !event.ctrlKey && !event.metaKey
            ? event.key
            : core.encodeKey(key);
        if (data) {
          core.scrollToBottom();
          send(data);
          invalidate();
        }
      }
      if (event.action === "paste" && operable) {
        core.scrollToBottom();
        send(core.encodePaste(event.text));
        invalidate();
      }
    },
  };
  watch.terminal = panel;
  signal.addEventListener("abort", () => clearTimeout(timer), { once: true });
  void run(
    Effect.gen(function* () {
      loading ??= loadRuntime().catch((e) => {
        loading = undefined;
        throw e;
      });
      const runtime = yield* Effect.promise(() => loading);
      core = GhosttyTerminalCore.fromRuntime(runtime, cols, rows, 7.23, 16, theme(dark), (data) =>
        send(data, true),
      );
      rpc = yield* openEnvironmentRpc(owner);
      const thread = state.projection.thread;
      const cwd = thread.worktreePath ?? state.projectRoot;
      reportedGrid = { cols, rows };
      const stream = operable
        ? rpc[WS_METHODS.terminalAttach]({
            ...id,
            cwd,
            worktreePath: thread.worktreePath,
            cols,
            rows,
            providerInstanceId: thread.providerInstanceId,
            restartIfNotRunning: true,
            ...(state.projectRoot && {
              env: projectScriptRuntimeEnv({
                project: { cwd: state.projectRoot },
                worktreePath: thread.worktreePath,
              }),
            }),
            maxHistoryBytes: DEFAULT_MAX_TERMINAL_BUFFER_BYTES,
          })
        : rpc[WS_METHODS.terminalObserve](id);
      let buffer = nextTerminalAttachSeedState();
      let cursor = INITIAL_TERMINAL_OUTPUT_CURSOR;
      yield* stream.pipe(
        Stream.runForEach((event) =>
          Effect.sync(() => {
            const previous = buffer;
            buffer = applyTerminalAttachStreamEvent(buffer, event);
            status = buffer.status;
            error = buffer.error;
            sharedGrid = buffer.grid;
            if (sharedGrid && previous.grid !== sharedGrid) {
              core.resize(sharedGrid.cols, sharedGrid.rows, 7.23, 16);
              force = true;
            } else if (!sharedGrid && (event.type === "snapshot" || event.type === "restarted")) {
              core.resize(cols, rows, 7.23, 16);
              force = true;
            }
            if (previous.status !== "running" && status === "running") reportGrid();
            const update = readTerminalOutputUpdate(buffer.output, cursor);
            cursor = update.cursor;
            if (update.type === "reset") {
              core.resetAndWrite(update.data);
              force = true;
            }
            if (update.type === "append") core.write(update.data);
            invalidate();
          }),
        ),
      );
      if (!["closed", "exited", "error"].includes(status)) {
        status = "disconnected";
        invalidate();
      }
    }).pipe(Effect.scoped),
  )
    .catch(() => {
      status = "disconnected";
      error = operable
        ? "Couldn’t connect to the terminal. Reopen it to retry."
        : "No terminal session is available to view.";
      if (core) invalidate();
      else
        reply({
          status,
          error,
          rows: 0,
          background: rgb(theme(dark).background),
          cursor: null,
          patches: [],
        });
    })
    .finally(() => {
      rpc = undefined;
      if (signal.aborted) core?.dispose();
      else signal.addEventListener("abort", () => core?.dispose(), { once: true });
    });
}
