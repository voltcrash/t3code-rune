import * as NodePath from "@effect/platform-node/NodePath";
import { it, expect } from "@effect/vitest";
import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";

import { pairWithTarget, resolveFolder } from "./projects.ts";

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
          capabilities: { repositoryIdentity: true },
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
  await pairWithTarget(
    { httpBaseUrl: origin, credential: "fixture" },
    new AbortController().signal,
    http,
  );
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
