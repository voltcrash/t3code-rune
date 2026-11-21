import {
  stripDisplayedPlanMarkdown,
  proposedPlanTitle,
  proposedPlanPreviewEnd,
} from "@t3tools/shared/proposedPlanText";
import { msg } from "gt-react";

import { m, type MessageResolver } from "./i18n/messages";

export function buildCollapsedProposedPlanPreviewMarkdown(
  planMarkdown: string,
  options?: {
    maxLines?: number;
  },
  resolve: MessageResolver = m,
): string {
  const maxLines = options?.maxLines ?? 8;
  const body = stripDisplayedPlanMarkdown(planMarkdown);
  const end = proposedPlanPreviewEnd(body, maxLines);
  const preview = body
    .slice(0, end)
    .trimEnd()
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .join("\n");
  if (!preview) return proposedPlanTitle(planMarkdown) ?? resolve(msg("Plan preview unavailable."));
  return end === undefined ? preview : preview + "\n\n...";
}

export const PLAN_IMPLEMENTATION_PROMPT_PREFIX = "PLEASE IMPLEMENT THIS PLAN:\n";

export {
  buildProposedPlanMarkdownFilename,
  normalizePlanMarkdownForExport,
} from "@t3tools/shared/proposedPlanText";

export function downloadPlanAsTextFile(filename: string, contents: string): void {
  const blob = new Blob([contents], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  window.setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 0);
}
