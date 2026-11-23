import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import {
  EnvironmentId,
  ProjectId,
  IssueTrackerKind,
  WS_METHODS,
  type IssueListInput,
  type IssueListResult,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { expect, it, vi } from "vite-plus/test";
import { workspaceIssues } from "./issues.ts";

it("keeps a late page out of a new filter and continues the current tracker cursor", async () => {
  const session = {
    environmentId: EnvironmentId.make("issues"),
    httpBaseUrl: "https://example.test",
    token: "fixture",
    canReadIssues: true,
    http: layerRemoteHttpClient(globalThis.fetch),
  };
  const entered = Promise.withResolvers<void>();
  const first = Promise.withResolvers<IssueListResult>();
  const requests: IssueListInput[] = [];
  const result = (key: string, next?: string): IssueListResult => ({
    viewers: {},
    trackers: [
      {
        tracker: IssueTrackerKind.make("github"),
        host: "github.com",
        projectCount: 1,
        capabilities: null,
        configured: true,
        detail: null,
      },
    ],
    errors: [],
    nextCursors: next ? { "github.com example/app": next } : {},
    entries: [
      {
        tracker: IssueTrackerKind.make("github"),
        host: "github.com",
        scope: "example/app",
        key,
        displayKey: "#" + key,
        title: "Issue " + key,
        url: "https://github.com/example/app/issues/" + key,
        author: null,
        state: { category: "closed", name: "Closed" },
        createdAt: "2026-01-01T10:00:00.000Z",
        updatedAt: "2026-01-01T10:00:00.000Z",
        closedAt: null,
        assignees: [],
        labels: [],
        commentCount: 0,
        projectId: ProjectId.make("app"),
        projectTitle: "App",
      },
    ],
  });
  const rpc = {
    [WS_METHODS.issuesList]: (input: IssueListInput) => {
      requests.push(input);
      if (input.state === "open")
        return Effect.promise(() => {
          entered.resolve();
          return first.promise;
        });
      const page = input.cursors ? result("3") : result("2", "next");
      const tracker = { ...page.trackers[0]!, host: "git.example.com" };
      const entry = {
        ...page.entries[0]!,
        host: tracker.host,
        displayKey: "#enterprise",
        url: "https://git.example.com/org/app/issues/42",
        updatedAt: "2026-02-01T10:00:00.000Z",
        createdAt: "2025-12-01T10:00:00.000Z",
      };
      return Effect.succeed({
        ...page,
        trackers: [...page.trackers, tracker],
        entries: [...page.entries, entry],
      });
    },
  };
  const publish = vi.fn<Parameters<typeof workspaceIssues>[3]>();
  const page = workspaceIssues(session, new AbortController().signal, () => rpc, publish);
  const opening = page.request({ operation: 1, state: "open", query: "", action: "list" });
  await entered.promise;
  await page.request({ operation: 2, state: "closed", query: "", action: "list" });
  first.resolve(result("1"));
  await opening;
  expect(publish.mock.calls.map(([operation]) => operation)).toEqual([2]);
  await page.request({ operation: 3, state: "closed", query: "", action: "more" });
  expect(requests[2]?.cursors).toEqual({ "github.com example/app": "next" });
  expect(publish.mock.lastCall?.[1]?.rows.map((row) => row.key)).toEqual([
    "#enterprise",
    "#2",
    "#3",
  ]);
  await page.request({ operation: 4, state: "closed", query: "", action: "list", sort: "created" });
  expect(publish.mock.lastCall?.[1]?.rows.map((row) => row.key)).toEqual(["#2", "#enterprise"]);
});
