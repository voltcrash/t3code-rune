import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import {
  ProjectId,
  SourceControlProviderKind,
  type PullRequestListEntry,
  type PullRequestListInput,
  type PullRequestListResult,
  WS_METHODS,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { expect, it } from "vite-plus/test";
import { workspacePullRequests, type PullRequestRpc } from "./pullRequests.ts";

const entry = (number: number, author: string, isDraft = false): PullRequestListEntry => ({
  provider: SourceControlProviderKind.make("github"),
  host: "github.com",
  projectId: ProjectId.make("project"),
  projectTitle: "Project",
  repository: "acme/app",
  number,
  title: `Change ${number}`,
  url: `https://github.com/acme/app/pull/${number}`,
  author: { login: author, name: null, avatarUrl: null },
  headBranch: `branch-${number}`,
  baseBranch: "main",
  state: "open",
  isDraft,
  mergeability: "mergeable",
  additions: 0,
  deletions: 0,
  createdAt: "2026-11-20T00:00:00.000Z",
  updatedAt: `2026-11-2${number}T00:00:00.000Z`,
  viewerReviewRequested: false,
  labels: [],
});

it("groups the viewer's own pull requests first, fills in line counts, and re-sorts locally", async () => {
  const listed: PullRequestListInput[] = [];
  const result = (entries: PullRequestListEntry[]): PullRequestListResult => ({
    viewers: { "github.com": "me" },
    providers: [],
    entries,
    errors: [],
    truncated: false,
    nextCursors: {},
  });
  const rpc: PullRequestRpc = {
    [WS_METHODS.pullRequestsList]: (input) => {
      listed.push(input);
      return Effect.succeed(
        result(
          input.involvement === "authored"
            ? [entry(2, "me", true)]
            : input.involvement === "reviewing"
              ? []
              : [entry(1, "octocat"), entry(2, "me", true)],
        ),
      );
    },
    [WS_METHODS.pullRequestsListStats]: ({ refs }) =>
      Effect.succeed({
        stats: refs.map((ref) => ({ ...ref, additions: ref.number * 10, deletions: 3 })),
      }),
    [WS_METHODS.pullRequestsInvalidate]: () => Effect.void,
  };
  const published: Array<{ groups?: Array<{ key: string; rows: Array<object> }> }> = [];
  const done = Promise.withResolvers<void>();
  const list = workspacePullRequests(
    {
      httpBaseUrl: "https://example.test",
      token: "fixture",
      canReadPullRequests: true,
      http: layerRemoteHttpClient(globalThis.fetch),
    },
    new AbortController().signal,
    () => rpc,
    (_operation, reply) => {
      published.push(reply);
      if (published.length === 2) done.resolve();
    },
  );
  await list.request({
    operation: 1,
    action: "list",
    state: 1,
    involvement: 0,
    sort: 2,
    query: "",
  });
  await done.promise;

  expect(listed.map((input) => input.involvement)).toEqual(["all", "authored", "reviewing"]);
  expect(published[0]?.groups?.map((group) => group.key)).toEqual(["authored", "others"]);
  expect(published[0]?.groups?.[0]?.rows[0]).toMatchObject({
    number: 2,
    state: "draft",
    measured: false,
  });
  expect(published[1]?.groups?.[1]?.rows[0]).toMatchObject({
    number: 1,
    additions: 10,
    deletions: 3,
    measured: true,
  });

  await list.request({
    operation: 2,
    action: "sort",
    state: 1,
    involvement: 0,
    sort: 5,
    query: "",
  });
  expect(listed).toHaveLength(3);
  expect(published[2]?.groups?.[1]?.rows[0]).toMatchObject({ number: 1, additions: 10 });

  await list.request({
    operation: 3,
    action: "list",
    state: 1,
    involvement: 0,
    sort: 0,
    query: "label:bug fix",
  });
  expect(listed.at(-1)).toMatchObject({ query: "fix", filters: { labels: [["bug"]] } });
});
