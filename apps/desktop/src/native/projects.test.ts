import * as NodePath from "@effect/platform-node/NodePath";
import { it, expect } from "@effect/vitest";
import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";

import { resolveFolder } from "./projects.ts";

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
