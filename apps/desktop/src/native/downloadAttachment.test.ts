import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { FetchHttpClient } from "effect/http";
import { downloadAttachment, downloadDestination, downloadQueueKey } from "./downloadAttachment.ts";

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
    const save = (content: string) =>
      downloadAttachment(
        "https://paired.example.test",
        "/api/assets/attachment/report",
        3,
        path,
      ).pipe(
        Effect.provide(FetchHttpClient.layer),
        Effect.provideService(FetchHttpClient.Fetch, () => Promise.resolve(new Response(content))),
      );
    for (const incomplete of ["no", "excess"]) {
      expect((yield* Effect.result(save(incomplete)))._tag).toBe("Failure");
      expect(yield* fs.readFileString(path)).toBe("original");
      expect(yield* fs.readDirectory(directory)).toEqual(["report.txt"]);
    }
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
