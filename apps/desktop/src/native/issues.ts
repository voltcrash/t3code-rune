import {
  continueIssuePage,
  groupIssues,
  holdIssuePages,
  type HeldIssuePage,
} from "@t3tools/client-runtime/issue-list";
import { sourceControlClients } from "@t3tools/client-runtime/source-control-clients";
import { sourceControlIconBody } from "./sourceControlIcon.ts";
import { issueTrackerDetailText } from "@t3tools/client-runtime/state/issue-trackers";
import {
  type IssueListInput,
  type IssueListResult,
  type IssueListSort,
  type IssueListState,
  WS_METHODS,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function issueLabels() {
  return {
    detail_close: m(msg("Close")),
    detail_refresh: m(msg("Refresh issue")),
    older: m(msg("Load older comments")),
    browser: m(msg("Open in browser")),
    description: m(msg("Description")),
    labels: m(msg("Labels")),
    none: m(msg("None")),
    assignees: m(msg("Assignees")),
    title: m(msg("Issues", { $context: "sidebar link to the list of issues" })),
    search: m(msg("Search issues")),
    open: m(msg("Open", { $context: "issue state" })),
    closed: m(msg("Closed", { $context: "issue state" })),
    all: m(msg("All", { $context: "issue filter" })),
    refresh: m(msg("Refresh")),
    filters: m(msg("Filters")),
    sort: m(msg("Sort")),
    updated: m(msg("Recently updated")),
    newest: m(msg("Newest", { $context: "sort order" })),
    back: m(msg("Back")),
    unconfigured: m(msg("No trackers to read")),
    add_project: m(msg("Add a project whose remote is on a supported tracker.")),
    more: m(msg("Load more")),
    loading: m(msg("Loading…")),
    empty: m(msg("No issues under these filters.")),
    no_match: m(msg("Nothing matches that.")),
    tracker: m(msg("Filter by tracker")),
    ago: m(msg("{age} ago", { age: "{age}" })),
    just_now: m(msg("just now")),
  };
}

type IssueSession = Pick<
  Session,
  "httpBaseUrl" | "token" | "http" | "environmentId" | "canReadIssues"
>;
type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type ListError = Effect.Error<ReturnType<EnvironmentRpc[typeof WS_METHODS.issuesList]>>;

interface ListRpc {
  readonly [WS_METHODS.issuesList]: (
    input: IssueListInput,
  ) => Effect.Effect<IssueListResult, ListError>;
}

interface IssueRequest {
  readonly operation: number;
  readonly state: IssueListState;
  readonly query: string;
  readonly sort?: IssueListSort;
  readonly host?: string;
  readonly action: "list" | "more" | "refresh" | "search";
}

/** The tracker filter's choices, like the web's `hostOptionsFor`: one per host, configured first. */
function hostOptions(page: HeldIssuePage, chosen: string) {
  const hosts = new Map<string, (typeof page.answer.trackers)[number]>();
  const ordered = [...page.answer.trackers].sort((a, b) =>
    `${a.tracker} ${a.host}`.localeCompare(`${b.tracker} ${b.host}`),
  );
  for (const tracker of ordered) {
    const existing = hosts.get(tracker.host);
    if (existing === undefined || (!existing.configured && tracker.configured))
      hosts.set(tracker.host, tracker);
  }
  return [
    // A remembered host no project is on any more stays listed, so the filter can be taken off.
    ...(chosen && !hosts.has(chosen) ? [{ host: chosen, label: chosen, icon: null }] : []),
    ...[...hosts.values()].map((tracker) => {
      const name = sourceControlClients.find(tracker.tracker)?.label ?? tracker.tracker;
      return {
        host: tracker.host,
        label: hosts.size > 1 && name === tracker.host ? tracker.host : `${name} (${tracker.host})`,
        icon: sourceControlIconBody(tracker.tracker),
      };
    }),
  ];
}

const read = Effect.fn("nativeDesktop.readIssues")(function* (
  session: IssueSession,
  input: IssueListInput,
  activeRpc: () => ListRpc | undefined,
) {
  const connection = activeRpc() ?? (yield* openEnvironmentRpc(session));
  return yield* connection[WS_METHODS.issuesList](input);
});

function issueView(
  session: IssueSession,
  page: HeldIssuePage,
  query: string,
  sort: IssueListSort,
  host: string,
) {
  // The answer names every tracker even when one host is chosen; the web groups only that one.
  const shown = host
    ? {
        ...page,
        answer: {
          ...page.answer,
          trackers: page.answer.trackers.filter((tracker) => tracker.host === host),
        },
      }
    : page;
  const groups = groupIssues(new Map([[session.environmentId, shown]]), query, [], sort);
  return {
    has_trackers: groups.length > 0,
    hosts: hostOptions(page, host),
    searching: query.length > 0,
    groups: groups.map((group) => {
      const name = sourceControlClients.find(group.tracker.tracker)?.label ?? group.tracker.tracker;
      const showScope = new Set(group.rows.map((row) => row.scope.toLowerCase())).size > 1;
      return {
        key: `${group.tracker.tracker} ${group.tracker.host}`,
        name,
        host: name.toLowerCase() === group.tracker.host ? null : group.tracker.host,
        projects: m(
          msg("{count, plural, one {{count} project} other {{count} projects}}", {
            count: group.tracker.projectCount,
          }),
        ),
        icon: sourceControlIconBody(group.tracker.tracker),
        notice: group.tracker.configured
          ? null
          : group.tracker.detail === null
            ? m(msg("This tracker cannot be read right now."))
            : issueTrackerDetailText(group.tracker.detail, m),
        rows: group.rows.map((row) => ({
          identity: row.url,
          reference: {
            projectId: row.projectId,
            tracker: row.tracker,
            host: row.host,
            scope: row.scope,
            key: row.key,
          },
          title: row.title,
          key: row.displayKey,
          author: row.author?.login ?? "ghost",
          scope: showScope ? row.scope : null,
          comments: row.commentCount,
          updated_at: Math.floor(Date.parse(row.updatedAt) / 1000) || 0,
          status:
            row.state.category === "open"
              ? "open"
              : !row.state.closeReason || row.state.closeReason === "completed"
                ? "completed"
                : "closed",
        })),
      };
    }),
    // Without each project's repository the worker cannot tell which tracker an error is from.
    errors: page.errors.map((error) => `${error.projectTitle}: ${error.message}`),
    more: Object.keys(page.nextCursors).length > 0,
  };
}

export function workspaceIssues(
  session: IssueSession,
  signal: AbortSignal,
  activeRpc: () => ListRpc | undefined,
  publish: (operation: number, result?: ReturnType<typeof issueView>, error?: string) => void,
) {
  let current: AbortController | undefined;
  let held:
    | {
        page: HeldIssuePage;
        query: string;
        state: IssueListState;
        sort: IssueListSort;
        host: string;
      }
    | undefined;
  function close() {
    current?.abort();
    current = undefined;
    held = undefined;
  }
  async function request({
    operation,
    state,
    query,
    action,
    sort = "updated",
    host = "",
  }: IssueRequest) {
    current?.abort();
    const controller = new AbortController();
    current = controller;
    const until = AbortSignal.any([signal, controller.signal]);
    const previous =
      held?.query === query && held.state === state && held.sort === sort && held.host === host
        ? held.page
        : undefined;
    if (!previous) held = undefined;
    try {
      if (
        !session.canReadIssues ||
        !["open", "closed", "all"].includes(state) ||
        query.length > 200 ||
        typeof host !== "string" ||
        host.length > 200 ||
        (action === "more" && !previous)
      ) {
        throw new Error("Unavailable issue list");
      }
      const result = await Effect.runPromise(
        read(
          session,
          {
            state,
            limit: 50,
            sort,
            ...(query && { query }),
            ...(host && { host }),
            ...(action === "more"
              ? { cursors: previous?.nextCursors }
              : { refresh: action === "refresh" }),
          },
          activeRpc,
        ).pipe(
          Effect.delay(action === "search" ? "300 millis" : "0 millis"),
          Effect.scoped,
          Effect.timeout("60 seconds"),
          Effect.provide(session.http),
        ),
        { signal: until },
      );
      if (until.aborted) return;
      const page =
        action === "more" && previous
          ? continueIssuePage(previous, result)
          : holdIssuePages([[session.environmentId, result]], new Map()).get(
              session.environmentId,
            )!;
      held = { page, query, state, sort, host };
      publish(operation, issueView(session, page, query, sort, host));
    } catch {
      if (!until.aborted) publish(operation, undefined, m(msg("Couldn’t load issues.")));
    }
  }
  return { request, close };
}
