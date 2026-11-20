import { recordedFileDiff } from "@t3tools/client-runtime/work-log/recorded-file-diff";
import { msg } from "gt-react";
import { m, type MessageResolver } from "../i18n/messages";

export function toolCallDiff(
  item: Parameters<typeof recordedFileDiff>[0],
  workspaceRoot?: string,
  resolve: MessageResolver = m,
) {
  const result = recordedFileDiff(item, workspaceRoot);
  return {
    ...result,
    content: result.content.map(({ side, text }) => ({
      label:
        side === "before"
          ? resolve(msg("Recorded content before"))
          : resolve(msg("Recorded content after")),
      text,
    })),
  };
}
