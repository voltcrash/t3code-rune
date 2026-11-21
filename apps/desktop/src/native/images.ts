import { PROJECT_FAVICON_MAX_SOURCE_BYTES } from "@t3tools/client-runtime/project-favicon-cache";
import { isProjectFaviconFallbackUrl } from "@t3tools/shared/projectFavicon";
import * as Schema from "effect/Schema";
import {
  type AssetResource,
  PROVIDER_SEND_TURN_MAX_IMAGE_BYTES,
  WS_METHODS,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import { HttpClient } from "effect/http";
import { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

class ImageTooLarge extends Schema.TaggedError<ImageTooLarge>()("ImageTooLarge", {}) {}

type ImageResource = Extract<AssetResource, { _tag: "attachment" | "tool-output-image" }>;

export function imageKey(resource: ImageResource, revision?: string) {
  return JSON.stringify([resource, revision]);
}

export const readImage = Effect.fn("nativeDesktop.readImage")(function* (
  session: Session,
  resource: ImageResource | Extract<AssetResource, { _tag: "project-favicon" }>,
  activeRpc?: Effect.Success<ReturnType<typeof openEnvironmentRpc>>,
) {
  const rpc = activeRpc ?? (yield* openEnvironmentRpc(session));
  const signed = yield* rpc[WS_METHODS.assetsCreateUrl]({ resource });
  const favicon = resource._tag === "project-favicon";
  if (favicon && isProjectFaviconFallbackUrl(signed.relativeUrl)) return null;
  const maxBytes = favicon ? PROJECT_FAVICON_MAX_SOURCE_BYTES : PROVIDER_SEND_TURN_MAX_IMAGE_BYTES;
  const client = HttpClient.filterStatusOk(yield* HttpClient.HttpClient);
  const response = yield* client.get(new URL(signed.relativeUrl, session.httpBaseUrl));
  if (Number(response.headers["content-length"]) > maxBytes) return yield* new ImageTooLarge();
  const chunks: Uint8Array[] = [];
  let size = 0;
  yield* response.stream.pipe(
    Stream.runForEach((chunk) => {
      size += chunk.byteLength;
      if (size > maxBytes) return Effect.fail(new ImageTooLarge());
      chunks.push(chunk);
      return Effect.void;
    }),
  );
  return Buffer.concat(chunks, size).toString("base64");
});
