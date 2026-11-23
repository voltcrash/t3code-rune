import * as NodeSqlite from "node:sqlite";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import {
  readThreadLayout,
  setThreadDetailsVisibility,
  setThreadPanelWidth,
} from "./threadDetails.ts";

it.effect("remembers layout separately for each environment, thread, and draft", () =>
  Effect.gen(function* () {
    const dir = yield* (yield* FileSystem.FileSystem).makeTempDirectoryScoped();
    const database = (yield* Path.Path).join(dir, "layout.sqlite");
    const open = () => new NodeSqlite.DatabaseSync(database);
    setThreadDetailsVisibility(open, "remote", "one", false, false);
    setThreadDetailsVisibility(open, "local", "one", true, false);
    setThreadDetailsVisibility(open, "local", "one", false, false);
    setThreadDetailsVisibility(open, "local", "one", false, true);
    setThreadPanelWidth(open, "remote", "one", 643);
    setThreadPanelWidth(open, "local", "one", 440);
    setThreadPanelWidth(open, "local", "one", 510);
    setThreadPanelWidth(open, "local", "two", 360);
    expect(readThreadLayout(open, "remote")).toEqual({
      widths: { one: 643 },
      hidden: { threads: ["one"], drafts: [] },
    });
    expect(readThreadLayout(open, "local")).toEqual({
      widths: { one: 510, two: 360 },
      hidden: { threads: [], drafts: ["one"] },
    });
  }).pipe(Effect.provide(NodeServices.layer)),
);
