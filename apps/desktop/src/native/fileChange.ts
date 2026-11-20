import { formatDiffCount } from "@t3tools/client-runtime/diff-count";
import { formatWorkspaceRelativePath } from "@t3tools/client-runtime/file-path-display";
import { turnItemDetailRevision } from "@t3tools/client-runtime/work-log/item-detail";
import { recordedFileDiff } from "@t3tools/client-runtime/work-log/recorded-file-diff";
import {
  ORCHESTRATION_V2_WS_METHODS,
  type OrchestrationV2TurnItem,
  type ThreadId,
  type TurnItemId,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function fileChangeRow(
  key: string,
  item: Extract<OrchestrationV2TurnItem, { type: "file_change" }>,
  root?: string,
) {
  const display = (path: string) => formatWorkspaceRelativePath(path, root);
  const changes = item.changes ?? [];
  const path = display(changes[0]?.path ?? item.fileName);
  const caption = changes.length > 1 ? path + " +" + (changes.length - 1) + " more" : path;
  const records = changes.map((change) => {
    const kind = [change.fileType, change.mimeType].filter(Boolean).join(", ");
    return (
      change.operation +
      " " +
      (change.oldPath ? display(change.oldPath) + " → " : "") +
      display(change.path) +
      (kind ? " (" + kind + ")" : "")
    );
  });
  const hasCounts = item.additions !== undefined || item.deletions !== undefined;
  return {
    key,
    role: "file" as const,
    caption,
    body: [
      display(item.fileName),
      ...(item.status === "failed" && item.diffStr?.trim() ? [item.diffStr] : []),
      ...records,
    ].join("\n"),
    attachments: [],
    failed: item.status === "failed",
    file: {
      revision: turnItemDetailRevision(item),
      added: hasCounts ? formatDiffCount(item.additions ?? 0) : undefined,
      removed: hasCounts ? formatDiffCount(item.deletions ?? 0) : undefined,
    },
  };
}

export const readFileDiff = Effect.fn("nativeDesktop.readFileDiff")(function* (
  session: Session,
  input: { threadId: ThreadId; itemId: TurnItemId },
  root?: string,
) {
  const rpc = yield* openEnvironmentRpc(session);
  const { item } = yield* rpc[ORCHESTRATION_V2_WS_METHODS.getTurnItem](input);
  if (item?.type !== "file_change") return { message: "This file change is unavailable." };
  return { ...recordedFileDiff(item, root), count: item.changes?.length ?? 0 };
});
