import {
  continueIssuePage,
  groupIssues,
  holdIssuePages,
  type HeldIssuePage,
} from "@t3tools/client-runtime/issue-list";
import { sourceControlClients } from "@t3tools/client-runtime/source-control-clients";
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

/** A host's mark as an icon body native can draw, from the same artwork the web draws. */
function trackerIcon(tracker: string) {
  const icon = sourceControlClients.find(tracker)?.icon;
  if (!icon) return null;
  const [x = 0, y = 0, width = 0, height = 0] = icon.viewBox.split(/\s+/u).map(Number);
  if (!(width > 0 && height > 0)) return null;
  // Rune draws a body in a square viewBox from the origin, so centre the mark in one.
  const view = Math.max(width, height);
  const shift = `translate(${(view - width) / 2 - x} ${(view - height) / 2 - y})`;
  const paint = (value: string | { readonly gradient: string } | undefined) =>
    value === undefined ? "none" : typeof value === "string" ? value : `url(#${value.gradient})`;
  const defs = (icon.gradients ?? [])
    .map(
      (g) =>
        `<linearGradient id="${g.id}" x1="${g.x1}" y1="${g.y1}" x2="${g.x2}" y2="${g.y2}" gradientUnits="userSpaceOnUse"${g.transform ? ` gradientTransform="${g.transform}"` : ""}>${g.stops
          .map(
            (stop) =>
              `<stop offset="${stop.offset}" stop-color="${stop.color}"${stop.opacity === undefined ? "" : ` stop-opacity="${stop.opacity}"`}/>`,
          )
          .join("")}</linearGradient>`,
    )
    .join("");
  const paths = icon.paths
    .map(
      (path) =>
        `<path d="${path.d}" fill="${paint(path.fill)}"${path.fillRule ? ` fill-rule="${path.fillRule}"` : ""}${path.fillOpacity === undefined ? "" : ` fill-opacity="${path.fillOpacity}"`}${path.stroke ? ` stroke="${path.stroke}"` : ""}${path.strokeWidth === undefined ? "" : ` stroke-width="${path.strokeWidth}"`}/>`,
    )
    .join("");
  return {
    name: `tracker-${tracker}`,
    view,
    body: `${defs ? `<defs>${defs}</defs>` : ""}<g transform="${shift}">${paths}</g>`,
  };
}

function issueView(session: IssueSession, page: HeldIssuePage, query: string, sort: IssueListSort) {
  const groups = groupIssues(new Map([[session.environmentId, page]]), query, [], sort);
  return {
    has_trackers: groups.length > 0,
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
        icon: trackerIcon(group.tracker.tracker),
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
