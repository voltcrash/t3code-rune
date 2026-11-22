import { expect, it } from "@effect/vitest";
import { vi } from "vite-plus/test";
import * as Effect from "effect/Effect";
import { FetchHttpClient } from "effect/http";
import { readRegistryIcon } from "./images.ts";

const url = "https://cdn.agentclientprotocol.com/registry/v1/latest/test-agent.svg";

it.effect(
  "reads official registry artwork without following redirects or supplying credentials",
  () =>
    Effect.gen(function* () {
      const request = vi
        .fn<typeof fetch>()
        .mockResolvedValue(new Response("badge", { headers: { "content-type": "image/svg+xml" } }));
      const read = (url: string) =>
        readRegistryIcon(url).pipe(Effect.provideService(FetchHttpClient.Fetch, request));
      expect(yield* read("https://example.org/agent.svg")).toBeNull();
      expect(request).not.toHaveBeenCalled();
      expect(yield* read(url)).toBe(Buffer.from("badge").toString("base64"));
      expect(request).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({
          redirect: "error",
          credentials: "omit",
          referrerPolicy: "no-referrer",
        }),
      );
      expect(String(request.mock.calls[0]?.[0])).toBe(url);
    }),
);

it.effect("limits an icon whose response omits its size", () =>
  Effect.gen(function* () {
    const request = vi
      .fn<typeof fetch>()
      .mockResolvedValue(
        new Response(new Uint8Array(512 * 1024 + 1), { headers: { "content-type": "image/png" } }),
      );
    const result = yield* Effect.flip(readRegistryIcon(url)).pipe(
      Effect.provideService(FetchHttpClient.Fetch, request),
    );
    expect(result._tag).toBe("ImageTooLarge");
  }),
);

it.effect("releases rejected responses before returning the image transfer slot", () =>
  Effect.gen(function* () {
    for (const init of [
      { status: 404, headers: { "content-type": "image/png" } },
      { headers: { "content-type": "text/plain" } },
      { headers: { "content-type": "image/png", "content-length": "524289" } },
    ]) {
      const request = vi
        .fn<typeof fetch>()
        .mockResolvedValue(new Response(new ReadableStream(), init));
      yield* Effect.result(readRegistryIcon(url)).pipe(
        Effect.provideService(FetchHttpClient.Fetch, request),
      );
      expect(request.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);
    }
  }),
);
