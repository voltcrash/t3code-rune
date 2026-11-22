import { resolveSymlinkTarget } from "@t3tools/shared/symlink";
import * as Option from "effect/Option";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Stream from "effect/Stream";
import { FetchHttpClient, HttpClient } from "effect/http";
import { AttachmentUnavailable } from "./attachments.ts";

export const downloadDestination = Effect.fn("nativeDesktop.downloadDestination")(function* (
  destination: string,
) {
  const path = yield* Path.Path;
  const fs = yield* FileSystem.FileSystem;
  let ancestor = yield* resolveSymlinkTarget(destination);
  const missing: string[] = [];
  while (true) {
    const resolved = yield* fs.realPath(ancestor).pipe(
      Effect.asSome,
      Effect.catchIf(
        (error) => error.reason._tag === "NotFound",
        () => Effect.succeedNone,
      ),
    );
    if (Option.isSome(resolved)) return path.join(resolved.value, ...missing.reverse());
    const parent = path.dirname(ancestor);
    if (parent === ancestor) return yield* fs.realPath(ancestor);
    missing.push(path.basename(ancestor));
    ancestor = parent;
  }
});

export const downloadQueueKey = Effect.fn("nativeDesktop.downloadQueueKey")(function* (
  destination: string,
  active: Iterable<string>,
) {
  const path = yield* Path.Path;
  const fs = yield* FileSystem.FileSystem;
  const folded = destination.normalize("NFD").toLowerCase();
  for (const other of active) {
    if (other === destination) return other;
    if (other.normalize("NFD").toLowerCase() !== folded) continue;
    let parent = path.dirname(destination);
    let otherParent = path.dirname(other);
    while (!(yield* fs.exists(parent))) {
      const next = path.dirname(parent);
      if (parent === next) return yield* fs.realPath(parent);
      parent = next;
      otherParent = path.dirname(otherParent);
    }
    const relative = path.relative(parent, destination);
    const candidate = path.relative(otherParent, other);
    // Ask the destination filesystem whether the two new names denote one entry.
    const probe = yield* fs.makeTempDirectoryScoped({ directory: parent, prefix: ".t3-save-" });
    const first = path.join(probe, relative);
    yield* fs.makeDirectory(path.dirname(first), { recursive: true });
    yield* fs.writeFile(first, new Uint8Array());
    if (yield* fs.exists(path.join(otherParent, path.basename(probe), candidate))) return other;
  }
  return destination;
}, Effect.scoped);

export const attachmentResponse = Effect.fn("nativeDesktop.attachmentResponse")(function* (
  baseUrl: string,
  relativeUrl: string,
  range?: string,
) {
  const url = new URL(relativeUrl, baseUrl);
  const invalid = new AttachmentUnavailable({ message: "The attachment is unavailable." });
  if (url.origin !== new URL(baseUrl).origin) return yield* invalid;
  const client = HttpClient.withScope(HttpClient.filterStatusOk(yield* HttpClient.HttpClient));
  return yield* client
    .get(url, { headers: range ? { Range: range } : {} })
    .pipe(Effect.provideService(FetchHttpClient.RequestInit, { redirect: "error" }));
});

export const downloadAttachment = Effect.fn("nativeDesktop.downloadAttachment")(function* (
  baseUrl: string,
  relativeUrl: string,
  expectedBytes: number,
  destination: string,
) {
  const invalid = new AttachmentUnavailable({ message: "The attachment is unavailable." });
  const response = yield* attachmentResponse(baseUrl, relativeUrl);
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
