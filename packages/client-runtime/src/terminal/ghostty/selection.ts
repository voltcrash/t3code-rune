import type { GhosttyTerminalCore } from "./core.ts";

type Cell = { x: number; y: number };

export class GhosttySelection {
  private readonly core: GhosttyTerminalCore;
  end: Cell | null = null;
  anchor: Cell | null = null;
  mode: "cell" | "word" | "line" = "cell";
  private base: {
    start: Cell;
    end: Cell;
  } | null = null;
  moved = false;

  constructor(core: GhosttyTerminalCore) {
    this.core = core;
  }

  selectAll(): void {
    const range = this.core.selectAll();
    this.anchor = range?.start ?? null;
    this.end = range ? this.core.screenPointToViewport(range.end.x, range.end.y) : null;
    this.mode = "cell";
    this.base = null;
  }

  clear(): void {
    this.core.clearSelection();
    this.end = null;
    this.anchor = null;
    this.mode = "cell";
    this.base = null;
  }

  begin(cell: Cell, clickCount: number): void {
    this.moved = false;
    this.mode = clickCount >= 3 ? "line" : clickCount === 2 ? "word" : "cell";
    const range =
      this.mode === "line"
        ? this.core.selectLine(cell.x, cell.y)
        : this.mode === "word"
          ? this.core.selectWord(cell.x, cell.y)
          : null;
    if (range) {
      this.base = range.screen;
      this.end = range.viewport.end;
      this.anchor = range.screen.start;
    } else {
      this.mode = "cell";
      this.base = null;
      this.end = cell;
      const screen = this.core.viewportPointToScreen(cell.x, cell.y);
      this.anchor = screen;
      if (screen) {
        this.core.setSelection({ ...screen, tag: 2 }, { ...screen, tag: 2 });
      } else {
        this.core.setSelection(cell, cell);
      }
    }
  }

  extend(cell: Cell): boolean {
    if (this.anchor === null) return false;
    // Ghostty moves its selection with output; keep the drag on those rows too.
    const anchorScreen = this.core.selectionRange()?.start;
    if (!anchorScreen) return false;
    const rowDelta = anchorScreen.y - this.anchor.y;
    if (this.base && rowDelta !== 0) {
      const { start, end } = this.base;
      this.base = {
        start: { x: start.x, y: start.y + rowDelta },
        end: { x: end.x, y: end.y + rowDelta },
      };
    }
    this.moved = true;
    this.end = cell;
    const range =
      this.mode === "line"
        ? this.core.selectLine(cell.x, cell.y)
        : this.mode === "word"
          ? this.core.selectWord(cell.x, cell.y)
          : null;
    const cellScreen = this.core.viewportPointToScreen(cell.x, cell.y);
    if (cellScreen === null) return false;
    const base = this.base;
    const beforeBase =
      base !== null &&
      (cellScreen.y < base.start.y ||
        (cellScreen.y === base.start.y && cellScreen.x < base.start.x));
    const anchor = base === null ? anchorScreen : beforeBase ? base.end : base.start;
    const end = range === null ? cellScreen : beforeBase ? range.screen.start : range.screen.end;
    this.anchor = anchor;
    this.core.setSelection({ ...anchor, tag: 2 }, { ...end, tag: 2 });
    return true;
  }
}
