import { issueEventSentence } from "@t3tools/client-runtime/issue-presentation";
import { WS_METHODS, type IssueComment, type IssueDetail, type IssueRef } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import { currentLanguage, m } from "../i18n/catalog.ts";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

function trackerLink(raw: string) {
  const url = URL.parse(raw);
  return url && (url.protocol === "http:" || url.protocol === "https:") ? url.href : null;
}

type DetailRpc = Pick<
  Effect.Success<ReturnType<typeof openEnvironmentRpc>>,
  "issues.detail" | "issues.comments"
>;
type Reader = Pick<Session, "httpBaseUrl" | "token" | "http" | "canReadIssues">;

function commentView(comment: IssueComment, dates: Intl.DateTimeFormat) {
  const at = Date.parse(comment.createdAt);
  return {
    id: "comment:" + comment.id,
    at,
    caption: [comment.author?.login, dates.format(at)].filter(Boolean).join(" · "),
    body: comment.body,
  };
}

function documentView(detail: IssueDetail, dates: Intl.DateTimeFormat) {
  return {
    title: detail.title,
    key: detail.displayKey,
    scope: detail.scope,
    state: detail.state.name,
    byline: [
      detail.author?.login,
      dates.format(Date.parse(detail.createdAt)),
      !detail.writable && m(msg("Read-only")),
    ]
      .filter(Boolean)
      .join(" · "),
    activity_title: detail.commentCount
      ? m(msg("Activity ({count})", { count: detail.commentCount }))
      : m(msg("Activity")),
    open: detail.state.category === "open",
    url: trackerLink(detail.url),
    labels: detail.labels.map((label) => label.name),
    assignees: detail.assignees.map((actor) => actor.login),
    body: detail.body.trim() ? detail.body : m(msg("_No description provided._")),
    links: detail.linkedPullRequests.flatMap((link) => {
      const url = trackerLink(link.url);
      return url ? [{ title: link.repository + "#" + link.number + " · " + link.title, url }] : [];
    }),
    activity: [
      ...detail.comments.map((comment) => commentView(comment, dates)),
      ...detail.events.map((event) => ({
        id: "event:" + event.id,
        at: Date.parse(event.createdAt),
        caption: [
          event.actor?.login,
          issueEventSentence(event, m),
          dates.format(Date.parse(event.createdAt)),
        ]
          .filter(Boolean)
          .join(" · "),
        body: "",
      })),
    ],
    cursor: detail.commentsCursor,
  };
}

const readIssue = Effect.fn("nativeDesktop.readIssue")(function* (
  session: Reader,
  activeRpc: () => DetailRpc | undefined,
  reference: IssueRef,
  cursor?: string,
) {
  const rpc = activeRpc() ?? (yield* openEnvironmentRpc(session));
  const dates = new Intl.DateTimeFormat(currentLanguage(), { dateStyle: "medium" });
  if (cursor) {
    const page = yield* rpc[WS_METHODS.issuesComments]({ ...reference, cursor });
    return {
      older: page.comments.map((comment) => commentView(comment, dates)),
      cursor: page.nextCursor,
    };
  }
  return { document: documentView(yield* rpc[WS_METHODS.issuesDetail](reference), dates) };
});

export function issueDetailReader(
  session: Reader,
  signal: AbortSignal,
  activeRpc: () => DetailRpc | undefined,
  publish: (
    operation: number,
    reply: Effect.Success<ReturnType<typeof readIssue>> | { error: string },
  ) => void,
) {
  let pending: AbortController | undefined;
  return async (operation: number, reference?: IssueRef, cursor?: string) => {
    pending?.abort();
    if (!reference || signal.aborted) return;
    pending = new AbortController();
    const until = AbortSignal.any([signal, pending.signal]);
    try {
      if (!session.canReadIssues) throw new Error("Unavailable issue");
      const reply = await Effect.runPromise(
        readIssue(session, activeRpc, reference, cursor).pipe(
          Effect.scoped,
          Effect.timeout("60 seconds"),
          Effect.provide(session.http),
        ),
        { signal: until },
      );
      if (!until.aborted) publish(operation, reply);
    } catch {
      if (!until.aborted)
        publish(operation, {
          error: cursor
            ? m(msg("Older comments could not be read"))
            : m(msg("Could not load this issue")),
        });
    }
  };
}
