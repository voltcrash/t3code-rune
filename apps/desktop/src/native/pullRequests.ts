import {
  comparePullRequestsByRecency,
  groupPullRequestsByInvolvement,
  partitionPullRequestsWithPriority,
  PullRequestListSort,
  rankPullRequestMatches,
  sortPullRequestGroups,
  type PullRequestGroup,
} from "@t3tools/client-runtime/pull-request-list";
import {
  type PullRequestInvalidateInput,
  type PullRequestInvolvement,
  type PullRequestListEntry,
  type PullRequestListInput,
  type PullRequestListResult,
  type PullRequestListState,
  type PullRequestListStatsInput,
  type PullRequestListStatsResult,
  WS_METHODS,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { parsePullRequestQuery } from "@t3tools/client-runtime/pull-request-query";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function pullRequestLabels() {
  return {
    title: m(msg("Pull requests")),
    back: m(msg("Back")),
    search: m(msg("Search pull requests")),
    placeholder: m(msg("Search pull requests, or label:bug")),
    sort: m(msg("Sort")),
    filters: m(msg("Filters")),
    refresh: m(msg("Refresh pull requests")),
    sorts: [
      m(msg("Merge readiness")),
      m(msg("Blocked on me")),
      m(msg("Recently updated")),
      m(msg("Newest shown")),
      m(msg("Oldest shown")),
      m(msg("Largest shown")),
      m(msg("Smallest shown")),
    ],
    state: m(msg("State")),
    states: [
      m(msg("All", { $context: "pull request filter" })),
      m(msg("Open", { $context: "pull request state" })),
      m(msg("Closed", { $context: "pull request state" })),
      m(msg("Merged", { $context: "pull request state" })),
    ],
    involvement: m(msg("Involvement")),
    involvements: [
      m(msg("All", { $context: "pull request filter" })),
      m(msg("Reviewing", { $context: "pull request filter" })),
      m(msg("Authored", { $context: "pull request filter" })),
    ],
    empty: m(msg("No pull requests")),
    empty_detail: m(msg("Pull requests from every project in this workspace appear here.")),
    filtered: m(msg("Nothing under these filters")),
    filtered_detail: m(msg("Widen the state, involvement or project filter to see more.")),
    check: m(msg("Check again")),
    checking: m(msg("Checking...")),
    more: m(msg("Load more pull requests")),
    loading: m(msg("Loading...")),
    ago: m(msg("{age} ago", { age: "{age}" })),
    just_now: m(msg("just now")),
  };
}

type PullRequestSession = Pick<Session, "httpBaseUrl" | "token" | "http" | "canReadPullRequests">;
type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type RpcError<Method extends keyof EnvironmentRpc> = Effect.Error<
  ReturnType<EnvironmentRpc[Method]>
>;

export interface PullRequestRpc {
  readonly [WS_METHODS.pullRequestsList]: (
    input: PullRequestListInput,
  ) => Effect.Effect<PullRequestListResult, RpcError<typeof WS_METHODS.pullRequestsList>>;
  readonly [WS_METHODS.pullRequestsListStats]: (
    input: PullRequestListStatsInput,
  ) => Effect.Effect<PullRequestListStatsResult, RpcError<typeof WS_METHODS.pullRequestsListStats>>;
  readonly [WS_METHODS.pullRequestsInvalidate]: (
    input: PullRequestInvalidateInput,
  ) => Effect.Effect<void, RpcError<typeof WS_METHODS.pullRequestsInvalidate>>;
}

const PAGE_SIZE = 99;
const STATES = ["all", "open", "closed", "merged"] as const;
const INVOLVEMENTS = ["all", "reviewing", "authored"] as const;
const SORTS = PullRequestListSort.literals;

export interface PullRequestRequest {
  readonly operation: number;
  readonly action: "list" | "search" | "refresh" | "more" | "sort";
  readonly state: number;
  readonly involvement: number;
  readonly sort: number;
  readonly query: string;
}

const GROUP_LABELS: Record<PullRequestGroup["key"], () => string> = {
  authored: () => m(msg("Authored")),
  reviewRequested: () => m(msg("Review requested")),
  others: () => m(msg("Others")),
};

/** The rows the native list draws, in the order and groups the web list shows. */
export function pullRequestView(
  groups: ReadonlyArray<PullRequestGroup>,
  stats: ReadonlyMap<string, { additions: number; deletions: number }>,
) {
  return groups.map((group) => ({
    key: group.key,
    label: group.label ? GROUP_LABELS[group.key]() : "",
    rows: group.entries.map((entry) => {
      const measured = stats.get(entry.url);
      return {
        url: entry.url,
        number: entry.number,
        title: entry.title,
        state: entry.isDraft && entry.state === "open" ? "draft" : entry.state,
        conflicting: entry.state === "open" && entry.mergeability === "conflicting",
        author: entry.author?.login ?? "ghost",
        repository: entry.repository,
        updated_at: Math.floor(Date.parse(entry.updatedAt) / 1000) || 0,
        additions: measured?.additions ?? entry.additions,
        deletions: measured?.deletions ?? entry.deletions,
        measured: measured !== undefined || entry.additions + entry.deletions > 0,
      };
    }),
  }));
}

function pullRequestGroups(input: {
  readonly involvement: PullRequestInvolvement;
  readonly text: string;
  readonly sort: PullRequestListSort;
  readonly list: PullRequestListResult;
  readonly partitions: Partitions;
  readonly measured: (entry: PullRequestListEntry) => boolean;
}) {
  const { involvement, text, sort, list, partitions } = input;
  const grouped: ReadonlyArray<PullRequestGroup> =
    involvement !== "all"
      ? [{ key: "others", label: "", entries: list.entries }]
      : partitions
        ? partitionPullRequestsWithPriority(list.entries, partitions[0], partitions[1])
        : groupPullRequestsByInvolvement(list.entries, list.viewers);
  return sortPullRequestGroups(grouped, sort, text, input.measured, involvement);
}

type Partitions =
  | readonly [ReadonlyArray<PullRequestListEntry>, ReadonlyArray<PullRequestListEntry>]
  | undefined;
type Stats = Map<string, { additions: number; deletions: number }>;

export function workspacePullRequests(
  session: PullRequestSession,
  signal: AbortSignal,
  activeRpc: () => PullRequestRpc | undefined,
  publish: (
    operation: number,
    reply: {
      groups?: ReturnType<typeof pullRequestView>;
      more?: boolean;
      filtered?: boolean;
      error?: string;
    },
  ) => void,
) {
  let current: AbortController | undefined;
  let held:
    | {
        readonly key: string;
        readonly list: PullRequestListResult;
        readonly partitions: Partitions;
        readonly stats: Stats;
      }
    | undefined;
  function close() {
    current?.abort();
    current = undefined;
    held = undefined;
  }
  async function request(input: PullRequestRequest) {
    current?.abort();
    const controller = new AbortController();
    current = controller;
    const until = AbortSignal.any([signal, controller.signal]);
    const state: PullRequestListState | undefined = STATES[input.state];
    const involvement: PullRequestInvolvement | undefined = INVOLVEMENTS[input.involvement];
    const sort = SORTS[input.sort];
    const query = typeof input.query === "string" ? input.query.trim() : "";
    const { text, filters } = parsePullRequestQuery(query);
    const key = JSON.stringify([state, involvement, query]);
    const previous = held?.key === key ? held : undefined;
    if (!previous) held = undefined;
    const run = <A, E>(effect: Effect.Effect<A, E, never>) =>
      Effect.runPromise(effect, { signal: until });
    const call = <A, E>(use: (rpc: PullRequestRpc) => Effect.Effect<A, E>) =>
      run(
        Effect.gen(function* () {
          const rpc = activeRpc() ?? (yield* openEnvironmentRpc(session));
          return yield* use(rpc);
        }).pipe(Effect.scoped, Effect.timeout("60 seconds"), Effect.provide(session.http)),
      );
    try {
      if (
        !session.canReadPullRequests ||
        !state ||
        !involvement ||
        !sort ||
        query.length > 200 ||
        ((input.action === "more" || input.action === "sort") && !previous)
      ) {
        throw new Error("Unavailable pull request list");
      }
      const show = (list: PullRequestListResult, partitions: Partitions, stats: Stats) =>
        publish(input.operation, {
          groups: pullRequestView(
            pullRequestGroups({
              involvement,
              text,
              sort,
              list,
              partitions,
              measured: (entry) => entry.additions + entry.deletions > 0 || stats.has(entry.url),
            }),
            stats,
          ),
          more: Object.keys(list.nextCursors).length > 0,
          filtered: state !== "open" || involvement !== "all" || query.length > 0,
        });
      // Sorting reorders the rows already read, as the web list does, instead of reading again.
      if (input.action === "sort" && previous) {
        show(previous.list, previous.partitions, previous.stats);
        return;
      }
      if (input.action === "search") {
        await run(Effect.sleep("300 millis"));
      }
      if (input.action === "refresh") {
        await call((rpc) => rpc[WS_METHODS.pullRequestsInvalidate]({}));
      }
      const base = { state, involvement, limit: PAGE_SIZE } as const;
      const scoped = Object.keys(filters).length > 0 ? { filters } : {};
      const page = await call((rpc) =>
        rpc[WS_METHODS.pullRequestsList]({
          ...base,
          ...scoped,
          ...(text && { query: text }),
          ...(input.action === "more" && previous ? { cursors: previous.list.nextCursors } : {}),
        }),
      );
      if (until.aborted) return;
      const continued = input.action === "more" && previous;
      const arrived = continued
        ? page.entries.filter(
            (entry) => !previous.list.entries.some((held) => held.url === entry.url),
          )
        : page.entries;
      const list: PullRequestListResult = {
        ...page,
        entries: [
          ...(continued ? previous.list.entries : []),
          ...rankPullRequestMatches([...arrived].sort(comparePullRequestsByRecency), text),
        ],
      };
      const stats: Stats = continued ? previous.stats : new Map();
      // Like the web list, priority reads only refine an unsearched "All" listing.
      const partitions: Partitions = continued
        ? previous.partitions
        : involvement === "all" && query.length === 0 && list.entries.length > 0
          ? await Promise.all([
              call((rpc) => rpc[WS_METHODS.pullRequestsList]({ ...base, involvement: "authored" })),
              call((rpc) =>
                rpc[WS_METHODS.pullRequestsList]({ ...base, involvement: "reviewing" }),
              ),
            ]).then(
              ([authored, reviewing]) => [authored.entries, reviewing.entries] as const,
              () => undefined,
            )
          : undefined;
      if (until.aborted) return;
      held = { key, list, partitions, stats };
      show(list, partitions, stats);
      const unmeasured = [...list.entries, ...(partitions?.[0] ?? []), ...(partitions?.[1] ?? [])]
        .filter((entry) => entry.additions + entry.deletions === 0 && !stats.has(entry.url))
        .slice(0, 500);
      if (unmeasured.length === 0) return;
      const answer = await call((rpc) =>
        rpc[WS_METHODS.pullRequestsListStats]({
          refs: unmeasured.map((entry) => ({
            projectId: entry.projectId,
            host: entry.host,
            repository: entry.repository,
            number: entry.number,
          })),
        }),
      ).catch(() => undefined);
      if (until.aborted || !answer) return;
      for (const stat of answer.stats) {
        const entry = unmeasured.find(
          (candidate) =>
            candidate.projectId === stat.projectId &&
            candidate.repository === stat.repository &&
            candidate.number === stat.number,
        );
        if (entry) stats.set(entry.url, { additions: stat.additions, deletions: stat.deletions });
      }
      show(list, partitions, stats);
    } catch {
      if (!until.aborted)
        publish(input.operation, { error: m(msg("Couldn’t load pull requests.")) });
    }
  }
  return { request, close };
}
