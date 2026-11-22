import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { FetchHttpClient } from "effect/http";
import { downloadAttachment } from "./downloadAttachment.ts";

it.effect("replaces the chosen file only after receiving the complete attachment", () =>
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const directory = yield* fs.makeTempDirectoryScoped();
    const path = (yield* Path.Path).join(directory, "report.txt");
    yield* fs.writeFileString(path, "original");
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
    yield* save("pdf");
    expect(yield* fs.readFileString(path)).toBe("pdf");
  }).pipe(Effect.provide(NodeServices.layer)),
);
