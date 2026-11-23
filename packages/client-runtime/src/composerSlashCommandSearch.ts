import {
  insertRankedSearchResult,
  normalizeSearchQuery,
  scoreQueryMatch,
} from "@t3tools/shared/searchRanking";

import type { ServerProviderSkill } from "@t3tools/contracts";
import { scoreProviderSkill } from "./providerSkillSearch.ts";

export type SlashSearchItem =
  | { type: "slash-command"; command: string; description: string }
  | {
      type: "provider-slash-command";
      provider: string;
      command: { name: string };
      description: string;
    }
  | { type: "skill"; provider: string; skill: ServerProviderSkill };

/** Provider commands, `/side`, `/btw` and `/clear` work only at the message start. */
export function slashCommandItemsForPromptPosition<T extends SlashSearchItem>(
  items: ReadonlyArray<T>,
  isAtPromptStart: boolean,
): T[] {
  if (isAtPromptStart) {
    return [...items];
  }
  return items.filter(
    (item) =>
      item.type === "skill" ||
      (item.type === "slash-command" &&
        item.command !== "side" &&
        item.command !== "btw" &&
        item.command !== "clear"),
  );
}

function scoreSlashCommandItem(item: SlashSearchItem, query: string): number | null {
  if (item.type === "skill") {
    if (query === "skill") {
      return 0;
    }
    const skillQuery = query.startsWith("skill:") ? query.slice("skill:".length) : query;
    const skillScore = skillQuery ? scoreProviderSkill(item.skill, skillQuery) : 0;
    if (skillScore !== null) {
      return skillScore;
    }
    return "skill".startsWith(query) ? Number.MAX_SAFE_INTEGER : null;
  }

  const primaryValue =
    item.type === "slash-command" ? item.command.toLowerCase() : item.command.name.toLowerCase();
  const description = item.description.toLowerCase();

  const scores = [
    scoreQueryMatch({
      value: primaryValue,
      query,
      exactBase: 0,
      prefixBase: 2,
      boundaryBase: 4,
      includesBase: 6,
      fuzzyBase: 100,
      boundaryMarkers: ["-", "_", "/"],
    }),
    scoreQueryMatch({
      value: description,
      query,
      exactBase: 20,
      prefixBase: 22,
      boundaryBase: 24,
      includesBase: 26,
    }),
  ].filter((score): score is number => score !== null);

  if (scores.length === 0) {
    return null;
  }

  return Math.min(...scores);
}

export function searchSlashCommandItems<T extends SlashSearchItem>(
  items: ReadonlyArray<T>,
  query: string,
  limit = Number.POSITIVE_INFINITY,
): T[] {
  const normalizedQuery = normalizeSearchQuery(query, { trimLeadingPattern: /^\/+/ });
  if (!normalizedQuery) {
    return items.slice(0, limit);
  }

  const ranked: Array<{
    item: T;
    score: number;
    tieBreaker: string;
  }> = [];

  for (const item of items) {
    const score = scoreSlashCommandItem(item, normalizedQuery);
    if (score === null) {
      continue;
    }

    insertRankedSearchResult(
      ranked,
      {
        item,
        score,
        tieBreaker:
          item.type === "slash-command"
            ? `0\u0000${item.command}`
            : item.type === "provider-slash-command"
              ? `1\u0000${item.command.name}\u0000${item.provider}`
              : `2\u0000${item.skill.name}\u0000${item.provider}`,
      },
      limit,
    );
  }

  return ranked.map((entry) => entry.item);
}
