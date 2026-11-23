import {
  deriveLatestThreadRun,
  deriveRunlessWorkStartedAt,
  activeThreadRun,
} from "@t3tools/client-runtime/state/thread-execution";
import { deriveTimelineEntriesFromVisibleTurnItems } from "@t3tools/client-runtime/work-log/timeline";
import { collapsedTurnLayout } from "@t3tools/client-runtime/work-log/turn-folds";
import type { OrchestrationV2ThreadProjection } from "@t3tools/contracts";

type Fold = { id: string; label: string; at: number; ranges: [number, number][] };

export function nativeTurnLayout(
  projection: OrchestrationV2ThreadProjection,
  indices: Map<string, number>,
) {
  const entries = deriveTimelineEntriesFromVisibleTurnItems({
    visibleTurnItems: projection.visibleTurnItems,
    optimisticMessages: [],
    attempts: projection.attempts,
    nodes: projection.nodes,
    plans: projection.plans,
  });
  const runningRunId = activeThreadRun(projection)?.id ?? null;
  const runlessWorkActive = deriveRunlessWorkStartedAt(projection) !== null;
  const { folds, terminalAssistantMessageIds, activeVisualResponseRunIds } = collapsedTurnLayout(
    entries,
    {
      latestRun: deriveLatestThreadRun(projection),
      runningRunId,
      isWorking: runningRunId !== null || runlessWorkActive,
      runlessWorkActive,
    },
  );
  const replies = entries.flatMap((entry) => {
    if (entry.kind !== "message") return [];
    const { message } = entry;
    return terminalAssistantMessageIds.has(message.id) &&
      !message.streaming &&
      message.text.trim().length > 0 &&
      !(message.runId == null ? runlessWorkActive : activeVisualResponseRunIds.has(message.runId))
      ? [message.id]
      : [];
  });
  const result: Fold[] = [];
  if (!folds.size) return { folds: result, replies };
  const positions = new Map<string, number>();
  const anchors = new Map<string, number>();
  let next = indices.size;
  for (let i = projection.visibleTurnItems.length - 1; i >= 0; i--) {
    const row = projection.visibleTurnItems[i]!;
    const item = row.item;
    const id =
      item.type === "assistant_message" || item.type === "user_message" ? item.messageId : item.id;
    const index = indices.get(JSON.stringify([row.sourceThreadId, row.sourceItemId]));
    if (index !== undefined) {
      positions.set(id, index);
      next = index;
    }
    if (folds.has(id)) anchors.set(id, next);
  }
  for (const fold of folds.values()) {
    const ranges: [number, number][] = [];
    for (const id of fold.hiddenEntryIds) {
      const index = positions.get(id);
      if (index === undefined) continue;
      const last = ranges.at(-1);
      if (last && last[1] === index) last[1]++;
      else ranges.push([index, index + 1]);
    }
    const at = anchors.get(fold.anchorEntryId);
    if (at !== undefined) result.push({ id: fold.runId, label: fold.label, at, ranges });
  }
  return { folds: result, replies };
}
