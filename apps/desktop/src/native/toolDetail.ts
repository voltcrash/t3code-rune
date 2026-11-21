import * as DateTime from "effect/DateTime";
import { imageKey } from "./images.ts";
import { formatWorkspaceRelativePath } from "@t3tools/client-runtime/file-path-display";
import { resolveExternalWebLink } from "@t3tools/shared/url";
import { isJavaScriptReplTool } from "@t3tools/shared/toolActivity";
import {
  formatWorkEntryLabel,
  inlineToolResult,
  projectedWorkEntry,
} from "@t3tools/client-runtime/work-log/timeline";
import {
  toolGroupAction,
  workEntryDisplayIndicatesToolFailure,
} from "@t3tools/client-runtime/work-log/presentation";
import {
  isJsonToolText,
  toolCallLines,
  turnItemDetailRevision,
  turnItemNeedsDetailFetch,
  turnItemOutputImages,
  turnItemOutputText,
} from "@t3tools/client-runtime/work-log/item-detail";
import {
  ORCHESTRATION_V2_WS_METHODS,
  type OrchestrationV2ProjectedTurnItem,
  type OrchestrationV2TurnItem,
  type ThreadId,
  type TurnItemId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function toolRow(key: string, row: OrchestrationV2ProjectedTurnItem, root?: string) {
  const result = inlineToolResult(row.item);
  if (result?.kind === "sent-message") {
    return { key, role: result.kind, body: result.text, attachments: [] };
  }
  const entry = projectedWorkEntry(row);
  return {
    key,
    role: "tool" as const,
    caption: formatWorkEntryLabel(entry, root),
    body: "",
    attachments: [],
    tool: { action: toolGroupAction(entry), revision: turnItemDetailRevision(row.item) },
    failed: workEntryDisplayIndicatesToolFailure(entry),
  };
}

type Block = {
  title?: string;
  text: string;
  language?: string | undefined;
  href?: string | undefined;
};

export const readToolDetail = Effect.fn("nativeDesktop.readToolDetail")(function* (
  session: Session,
  input: { threadId: ThreadId; itemId: TurnItemId },
  projected: OrchestrationV2TurnItem,
  root?: string,
) {
  const fetch = turnItemNeedsDetailFetch(projected);
  let item: OrchestrationV2TurnItem | null = projected;
  if (fetch) {
    const rpc = yield* openEnvironmentRpc(session);
    item = (yield* rpc[ORCHESTRATION_V2_WS_METHODS.getTurnItem](input)).item;
  }
  if (!item || item.type !== projected.type) return { message: "This tool call is unavailable." };
  const blocks: Block[] = [];
  if (item.type === "dynamic_tool") {
    const call = toolCallLines({ args: item.input });
    if (call.args) {
      for (const [title, text] of call.args) {
        const language =
          title === "html"
            ? "html"
            : title === "code" && isJavaScriptReplTool(item.toolName)
              ? "typescript"
              : isJsonToolText(text)
                ? "json"
                : undefined;
        blocks.push({ title, text, language });
      }
    }
    if (call.argsText)
      blocks.push({
        text: call.argsText,
        language: isJsonToolText(call.argsText) ? "json" : undefined,
      });
    const revision = DateTime.formatIso(item.updatedAt);
    const images = turnItemOutputImages(item).map((resource) =>
      imageKey({ ...resource, ...input }, revision),
    );
    const output = turnItemOutputText(item);
    if (output)
      blocks.push({ text: output, language: isJsonToolText(output) ? "json" : undefined });
    else if (fetch && !images.length) return { blocks, message: "No output." };
    return { blocks, images };
  } else if (item.type === "file_search") {
    if (item.pattern?.trim()) blocks.push({ text: item.pattern });
    for (const result of item.results ?? []) {
      const location = [
        formatWorkspaceRelativePath(result.fileName, root),
        result.line,
        result.column,
      ]
        .filter((part) => part !== undefined)
        .join(":");
      blocks.push({ title: location, text: result.preview ?? "" });
    }
  } else if (item.type === "web_search") {
    if (item.patterns?.length) blocks.push({ text: item.patterns.join("\n") });
    for (const result of item.results ?? []) {
      const href = resolveExternalWebLink(result.url)?.href;
      blocks.push({
        title: result.title ?? result.url ?? "Search result",
        href,
        text: result.snippet ?? "",
      });
    }
  }
  return { blocks };
});
