import {
  continueIssuePage,
  groupIssues,
  holdIssuePages,
  issueOrder,
  type HeldIssuePage,
} from "@t3tools/client-runtime/issue-list";
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
  };
}

function trackerLink(raw: string) {
  const url = URL.parse(raw);
  return url && (url.protocol === "http:" || url.protocol === "https:") ? url.href : null;
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
  readonly action: "list" | "more" | "refresh" | "search";
}

const read = Effect.fn("nativeDesktop.readIssues")(function* (
  session: IssueSession,
  input: IssueListInput,
  activeRpc: () => ListRpc | undefined,
) {
  const connection = activeRpc() ?? (yield* openEnvironmentRpc(session));
  return yield* connection[WS_METHODS.issuesList](input);
});

function issueView(session: IssueSession, page: HeldIssuePage, query: string, sort: IssueListSort) {
  const groups = groupIssues(new Map([[session.environmentId, page]]), query, [], sort);
  const entries = groups.flatMap((group) => group.rows).sort(issueOrder(sort));
  return {
    has_trackers: groups.length > 0,
    rows: entries.map((row) => ({
      identity: row.url,
      group: row.host,
      title: row.title,
      key: row.displayKey,
      context: [row.scope, row.state.name, row.author?.login].filter(Boolean).join(" · "),
      status:
        row.state.category === "open"
          ? "open"
          : !row.state.closeReason || row.state.closeReason === "completed"
            ? "completed"
            : "closed",
      url: trackerLink(row.url),
    })),
    notices: [
      ...page.answer.trackers.flatMap((t) =>
        !t.configured && t.detail ? [t.host + " · " + t.detail] : [],
      ),
      ...page.errors.map((e) => e.projectTitle + " · " + e.message),
    ],
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
    | { page: HeldIssuePage; query: string; state: IssueListState; sort: IssueListSort }
    | undefined;
  function close() {
    current?.abort();
    current = undefined;
    held = undefined;
  }
  async function request({ operation, state, query, action, sort = "updated" }: IssueRequest) {
    current?.abort();
    const controller = new AbortController();
    current = controller;
    const until = AbortSignal.any([signal, controller.signal]);
    const previous =
      held?.query === query && held.state === state && held.sort === sort ? held.page : undefined;
    if (!previous) held = undefined;
    try {
      if (
        !session.canReadIssues ||
        !["open", "closed", "all"].includes(state) ||
        query.length > 200 ||
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
      held = { page, query, state, sort };
      publish(operation, issueView(session, page, query, sort));
    } catch {
      if (!until.aborted) publish(operation, undefined, m(msg("Couldn’t load issues.")));
    }
  }
  return { request, close };
}
