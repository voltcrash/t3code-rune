import {
  GhosttyTerminalCore as Terminal,
  type GhosttyTheme,
} from "@t3tools/client-runtime/terminal/ghostty/core";
import { loadGhosttyRuntime } from "./runtime";

export * from "@t3tools/client-runtime/terminal/ghostty/core";

export class GhosttyTerminalCore extends Terminal {
  static async create(
    cols: number,
    rows: number,
    cellWidth: number,
    cellHeight: number,
    theme: GhosttyTheme,
    onPtyData: (data: string) => void,
  ) {
    return GhosttyTerminalCore.fromRuntime(
      await loadGhosttyRuntime(),
      cols,
      rows,
      cellWidth,
      cellHeight,
      theme,
      onPtyData,
    );
  }
}
