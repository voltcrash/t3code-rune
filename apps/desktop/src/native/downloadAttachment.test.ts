import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as TestClock from "effect/testing/TestClock";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { FetchHttpClient } from "effect/http";
import { downloadFile, downloadDestination, downloadQueueKey } from "./downloadAttachment.ts";
import { FILE_TEXT_PREVIEW_MAX_BYTES } from "@t3tools/shared/filePreview";
import { readAttachmentPreview } from "./attachmentPreview.ts";

it.effect("replaces the chosen file only after receiving the complete attachment", () =>
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const directory = yield* fs.makeTempDirectoryScoped();
    const path = (yield* Path.Path).join(directory, "report.txt");
    const paths = yield* Path.Path;
    const linkedDirectory = paths.join(directory, "linked");
    yield* fs.symlink(directory, linkedDirectory);
    const linkedFile = paths.join(directory, "alias.txt");
    yield* fs.symlink("report.txt", linkedFile);
    const realPath = paths.join(yield* fs.realPath(directory), "report.txt");
    expect(yield* downloadDestination(paths.join(linkedDirectory, "alias.txt"))).toBe(realPath);
    yield* fs.remove(linkedFile);
    yield* fs.remove(linkedDirectory);
    yield* fs.writeFileString(path, "original");
    const alternate = paths.join(directory, "REPORT.TXT");
    const sameEntry = yield* fs.exists(alternate);
    const upper = paths.join(directory, "NEW.TXT");
    const lower = paths.join(directory, "new.txt");
    expect(yield* downloadQueueKey(upper, [lower])).toBe(sameEntry ? lower : upper);
    const save = (
      content: string,
      size: number | { maxBytes: number } = 3,
      headers: Record<string, string> = {},
    ) =>
      downloadFile("https://paired.example.test", "/api/assets/attachment/report", size, path).pipe(
        Effect.provide(FetchHttpClient.layer),
        Effect.provideService(FetchHttpClient.Fetch, () =>
          Promise.resolve(new Response(content, { headers })),
        ),
      );
    for (const incomplete of ["no", "excess"]) {
      expect((yield* Effect.result(save(incomplete)))._tag).toBe("Failure");
      expect(yield* fs.readFileString(path)).toBe("original");
      expect(yield* fs.readDirectory(directory)).toEqual(["report.txt"]);
    }
    for (const [body, length] of [
      ["over budget", undefined],
      ["no", "3"],
    ] as const) {
      const headers = length ? { "content-length": length } : undefined;
      expect((yield* Effect.result(save(body, { maxBytes: 4 }, headers)))._tag).toBe("Failure");
      expect(yield* fs.readFileString(path)).toBe("original");
    }
    yield* save("file", { maxBytes: 4 });
    expect(yield* fs.readFileString(path)).toBe("file");
    yield* save("decoded", { maxBytes: 7 }, { "content-length": "27", "content-encoding": "gzip" });
    expect(yield* fs.readFileString(path)).toBe("decoded");
    const mixed = paths.join(directory, "Mixed");
    const alternateParent = paths.join(directory, "mixed");
    yield* fs.makeDirectory(mixed);
    const sharesParent = yield* fs.exists(alternateParent);
    if (!sharesParent) yield* fs.makeDirectory(alternateParent);
    const first = yield* downloadDestination(paths.join(mixed, "new.md"));
    const second = yield* downloadDestination(paths.join(alternateParent, "new.md"));
    expect(yield* downloadQueueKey(second, [first])).toBe(sharesParent ? first : second);
    yield* save("pdf");
    expect(yield* fs.readFileString(path)).toBe("pdf");
  }).pipe(Effect.provide(NodeServices.layer)),
);

it.effect("allows a progressing download and discards a stalled partial file", () =>
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const directory = yield* fs.makeTempDirectoryScoped();
    const path = (yield* Path.Path).join(directory, "report");
    yield* fs.writeFileString(path, "previous");
    for (const stalled of [true, false]) {
      const reads = Array.from({ length: 3 }, () =>
        Promise.withResolvers<ReadableStreamDefaultController<Uint8Array>>(),
      );
      let next = 0;
      const stream = new ReadableStream<Uint8Array>(
        {
          pull(controller) {
            reads[next++]!.resolve(controller);
          },
        },
        { highWaterMark: 0 },
      );
      const job = yield* downloadFile(
        "https://paired.example.test",
        "/report",
        { maxBytes: 2 },
        path,
      ).pipe(
        Effect.provide(FetchHttpClient.layer),
        Effect.provideService(FetchHttpClient.Fetch, async () => new Response(stream)),
        Effect.result,
        Effect.forkChild,
      );
      for (const read of reads.slice(0, stalled ? 1 : 2)) {
        const controller = yield* Effect.promise(() => read.promise);
        yield* TestClock.adjust("4 minutes");
        controller.enqueue(Uint8Array.of(65));
      }
      const controller = yield* Effect.promise(() => reads[stalled ? 1 : 2]!.promise);
      if (stalled) yield* TestClock.adjust("6 minutes");
      else controller.close();
      expect((yield* Fiber.join(job))._tag).toBe(stalled ? "Failure" : "Success");
      expect(yield* fs.readFileString(path)).toBe(stalled ? "previous" : "AA");
    }
  }).pipe(Effect.provide(NodeServices.layer)),
);

it.effect("limits text previews when an asset server ignores the byte range", () =>
  Effect.gen(function* () {
    const max = FILE_TEXT_PREVIEW_MAX_BYTES;
    let range: string | null = null;
    const bytes = new Uint8Array(max + 4).fill(97);
    bytes.set([0xe2, 0x82, 0xac], max - 1);
    const result = yield* readAttachmentPreview(
      "https://paired.example.test",
      "/asset",
      bytes.length,
    ).pipe(
      Effect.provide(FetchHttpClient.layer),
      Effect.provideService(FetchHttpClient.Fetch, (_url, init) => {
        range = new Headers(init?.headers).get("Range");
        return Promise.resolve(new Response(bytes));
      }),
    );
    expect(range).toBe(`bytes=0-${max}`);
    expect(result.truncated).toBe(true);
    expect(result.text.length).toBe(max - 1);
  }),
);
