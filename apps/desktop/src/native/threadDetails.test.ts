import * as NodeSqlite from "node:sqlite";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { hiddenThreadDetails, setThreadDetailsVisibility } from "./threadDetails.ts";

it.effect("remembers visibility separately for each environment, thread, and draft", () =>
  Effect.gen(function* () {
    const dir = yield* (yield* FileSystem.FileSystem).makeTempDirectoryScoped();
    const database = (yield* Path.Path).join(dir, "layout.sqlite");
    const open = () => new NodeSqlite.DatabaseSync(database);
    setThreadDetailsVisibility(open, "remote", "one", false, false);
    setThreadDetailsVisibility(open, "local", "one", true, false);
    setThreadDetailsVisibility(open, "local", "one", false, false);
    setThreadDetailsVisibility(open, "local", "one", false, true);
    expect(hiddenThreadDetails(open, "local")).toEqual({ threads: [], drafts: ["one"] });
    expect(hiddenThreadDetails(open, "remote")).toEqual({ threads: ["one"], drafts: [] });
  }).pipe(Effect.provide(NodeServices.layer)),
);
