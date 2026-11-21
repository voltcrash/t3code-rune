import { expect, it } from "vite-plus/test";
import {
  proposedPlanBodyStart,
  proposedPlanCanCollapse,
  proposedPlanPreviewEnd,
  stripDisplayedPlanMarkdown,
} from "./proposedPlanText.ts";

it("locates the same plan body and preview in the original Markdown across line endings", () => {
  for (const newline of ["\n", "\r\n"]) {
    const source = [
      "# 計画",
      "",
      "## Summary",
      "",
      "  First step  ",
      "",
      "第二步",
      "",
      "Third step",
      "",
    ].join(newline);
    const body = source.slice(proposedPlanBodyStart(source));
    expect(body).toBe(["  First step  ", "", "第二步", "", "Third step", ""].join(newline));
    expect(stripDisplayedPlanMarkdown(source)).toBe("  First step  \n\n第二步\n\nThird step");
    const end = proposedPlanPreviewEnd(body, 2);
    expect(body.slice(end).trim()).toBe("Third step");
    expect(proposedPlanPreviewEnd(body, 3)).toBeUndefined();
  }
  expect(stripDisplayedPlanMarkdown("#\r\nFirst step")).toBe("#\nFirst step");
  expect(proposedPlanCanCollapse("x".repeat(900))).toBe(false);
  expect(proposedPlanCanCollapse("x".repeat(901))).toBe(true);
  expect(proposedPlanCanCollapse("x\n".repeat(20))).toBe(true);
});
