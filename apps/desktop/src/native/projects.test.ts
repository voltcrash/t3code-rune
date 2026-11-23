import * as NodePath from "@effect/platform-node/NodePath";
import { it, expect } from "@effect/vitest";
import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";

import { WS_METHODS } from "@t3tools/contracts";
import {
  pairWithTarget,
  resolveFolder,
  resolveWorkspaceMarkdownLink,
  workspaceMarkdownLink,
  type Session,
} from "./projects.ts";

it("restricts the pairing credential exchange to the confirmed endpoint", async () => {
  const origin = "https://environment.example.test";
  let exchange: { url: string; redirect: RequestRedirect | undefined } | undefined;
  const http = layerRemoteHttpClient((input, init) => {
    const url = String(input);
    if (url === origin + "/.well-known/t3/environment") {
      return Promise.resolve(
        Response.json({
          environmentId: "environment-1",
          label: "Fixture",
          platform: { os: "linux", arch: "x64" },
          serverVersion: "1.0.0",
          capabilities: { repositoryIdentity: true, serverBrowser: true },
        }),
      );
    }
    exchange = { url, redirect: init?.redirect };
    return Promise.resolve(
      Response.json({
        access_token: "fixture",
        issued_token_type: "urn:ietf:params:oauth:token-type:access_token",
        token_type: "Bearer",
        expires_in: 3600,
        scope: "orchestration:read",
      }),
    );
  });
  const session = await pairWithTarget(
    { httpBaseUrl: origin, credential: "fixture" },
    new AbortController().signal,
    http,
  );
  expect(session.browserAvailable).toBe(true);
  expect(session.browserWritable).toBe(false);
  expect(session.canWriteGit).toBe(false);
  expect(session.canWriteSettings).toBe(false);
  expect(exchange).toEqual({ url: origin + "/oauth/token", redirect: "error" });
});

it.effect(
  "resolves drive and UNC paths for Windows and rejects paths relative to its current drive",
  () =>
    Effect.gen(function* () {
      const session = {
        httpBaseUrl: "https://example.invalid",
        token: "",
        platform: "windows" as const,
        canBrowse: false,
      };
      for (const [input, expected] of [
        ["C:/work/../repo", "C:\\repo"],
        ["//host/share/work/../repo", "\\\\host\\share\\repo"],
        ["//repo", null],
        ["\\\\repo", null],
        ["/repo", null],
        ["C:repo", null],
      ] as const) {
        const result = yield* resolveFolder(session, input).pipe(Effect.option);
        expect(Option.getOrNull(result)).toBe(expected);
      }
    }).pipe(Effect.provide([NodePath.layerWin32, layerRemoteHttpClient(globalThis.fetch)])),
);

it("opens Markdown citations at their line inside the active environment checkout", () => {
  expect(workspaceMarkdownLink("README.md:12", "/repo")).toEqual({ path: "README.md", line: 12 });
  expect(workspaceMarkdownLink("docs/a%20b.md#L38-L42", "/work/project")).toEqual({
    path: "docs/a b.md",
    line: 38,
  });
  expect(
    workspaceMarkdownLink("file:///D:/work/Project/README.md#L2", "D:\\work\\project"),
  ).toEqual({ path: "README.md", line: 2 });
  for (const href of [
    "../elsewhere/secret.md",
    "file:///elsewhere/secret.md",
    "https://example.test/guide.md",
    "javascript:void(0)",
  ]) {
    expect(workspaceMarkdownLink(href, "/work/project")).toBeNull();
  }
});

it.effect("opens the one workspace file a missing or bare citation path names", () =>
  Effect.gen(function* () {
    const searched: string[] = [];
    const files = new Set(["/repo/apps/web/src/a.ts"]);
    const rpc = {
      [WS_METHODS.filesystemGetMetadata]: ({ paths }: { paths: readonly string[] }) =>
        Effect.succeed({ entries: paths.map((p) => (files.has(p) ? { kind: "file" } : null)) }),
      [WS_METHODS.projectsSearchEntries]: ({ query }: { query: string }) => {
        searched.push(query);
        return Effect.succeed({
          entries: [
            { path: "src/foo.ts", kind: "file" },
            { path: "apps/web/src/b.ts", kind: "file" },
          ],
          truncated: false,
        });
      },
    } as unknown as Parameters<typeof resolveWorkspaceMarkdownLink>[3];
    const resolve = (href: string) =>
      resolveWorkspaceMarkdownLink({} as Session, href, "/repo", rpc);
    expect(yield* resolve("foo.ts:12")).toEqual({ path: "src/foo.ts", line: 12 });
    expect(yield* resolve("src/b.ts")).toEqual({ path: "apps/web/src/b.ts", line: undefined });
    expect(yield* resolve("apps/web/src/a.ts")).toEqual({
      path: "apps/web/src/a.ts",
      line: undefined,
    });
    expect(yield* resolve("missing.ts")).toEqual({ path: "missing.ts", line: undefined });
    expect(searched).toEqual(["foo.ts", "src/b.ts", "missing.ts"]);
  }).pipe(Effect.provide(layerRemoteHttpClient(globalThis.fetch))),
);
