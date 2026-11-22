import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Stream from "effect/Stream";
import { FetchHttpClient, HttpClient } from "effect/http";
import { AttachmentUnavailable } from "./attachments.ts";

export const downloadAttachment = Effect.fn("nativeDesktop.downloadAttachment")(function* (
  baseUrl: string,
  relativeUrl: string,
  expectedBytes: number,
  destination: string,
) {
  const url = new URL(relativeUrl, baseUrl);
  const invalid = new AttachmentUnavailable({ message: "The attachment is unavailable." });
  if (url.origin !== new URL(baseUrl).origin) return yield* invalid;
  const client = HttpClient.withScope(HttpClient.filterStatusOk(yield* HttpClient.HttpClient));
  const response = yield* client
    .get(url)
    .pipe(Effect.provideService(FetchHttpClient.RequestInit, { redirect: "error" }));
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const temporary = yield* fs.makeTempFileScoped({
    directory: path.dirname(destination),
    prefix: ".t3-attachment-",
  });
  let bytes = 0;
  yield* response.stream.pipe(
    Stream.tap((chunk) => {
      bytes += chunk.byteLength;
      return bytes > expectedBytes ? Effect.fail(invalid) : Effect.void;
    }),
    Stream.run(fs.sink(temporary)),
  );
  if (bytes !== expectedBytes) return yield* invalid;
  yield* fs.rename(temporary, destination);
}, Effect.scoped);
