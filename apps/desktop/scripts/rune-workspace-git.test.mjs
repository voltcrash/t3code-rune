import { ORCHESTRATION_V2_WS_METHODS, WS_METHODS } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Stream from "effect/Stream";
import { expect, it, vi } from "vite-plus/test";
import { openEnvironmentRpc } from "../src/native/environmentRpc.ts";
import { watchWorkspace } from "../src/native/workspaceFeed.ts";

vi.mock("../src/native/environmentRpc.ts", () => ({ openEnvironmentRpc: vi.fn() }));

it.each([true, false])(
  "initializes only the replayed checkout with branch-status capability %s",
  async (vcsBranchStatus) => {
    const shellStarted = Promise.withResolvers();
    const replay = Promise.withResolvers();
    const synchronized = Promise.withResolvers();
    const unavailable = Promise.withResolvers();
    const initialized = [];
    const follow = (snapshot) => Stream.make(snapshot).pipe(Stream.concat(Stream.never));
    const nonRepo = { refName: null, isRepo: false };
    openEnvironmentRpc.mockReturnValue(
      Effect.succeed({
        [ORCHESTRATION_V2_WS_METHODS.subscribeShell]: () => {
          shellStarted.resolve();
          return Stream.fromEffect(Effect.promise(() => replay.promise)).pipe(
            Stream.concat(follow({ kind: "synchronized" })),
          );
        },
        [WS_METHODS.subscribeVcsBranch]: () =>
          vcsBranchStatus
            ? follow(nonRepo)
            : Stream.fromEffect(Effect.sync(() => unavailable.resolve(false))).pipe(Stream.drain),
        [WS_METHODS.subscribeVcsStatus]: ({ includeRemote }) =>
          includeRemote === false
            ? follow({ _tag: "snapshot", local: nonRepo })
            : Stream.die("Unexpected remote polling"),
        [WS_METHODS.vcsInit]: ({ cwd }) => Effect.sync(() => initialized.push(cwd)),
      }),
    );
    const project = { id: "project", title: "Checkout", path: "/old" };
    const thread = { id: "thread", project: project.id };
    const controller = new AbortController();
    const noop = () => {};
    try {
      const workspace = watchWorkspace(
        { environmentId: "remote", canWriteGit: true, vcsBranchStatus, http: Layer.empty },
        {
          sequence: 1,
          clockOffset: 0,
          configLoaded: true,
          view: { projects: [project], threads: [thread] },
          visitRows: new Map(),
          legacyScripts: new Map(),
        },
        controller.signal,
        noop,
        noop,
        noop,
        noop,
        () => ({ reconnect: synchronized.resolve }),
        noop,
        noop,
        noop,
      );
      const select = (id, cwd, notify) =>
        workspace.git.select(id, { draft: false, target: thread.id, cwd }, notify);
      select(1, project.path, noop);
      await shellStarted.promise;
      expect(await workspace.git.initialize(1)).toBe(false);
      replay.resolve({
        kind: "project.updated",
        sequence: 2,
        project: { ...project, workspaceRoot: "/moved", scripts: [] },
      });
      await synchronized.promise;
      expect(await workspace.git.initialize(1)).toBe(false);
      const offered = Promise.withResolvers();
      select(2, "/moved", (nonRepo) => nonRepo && offered.resolve(true));
      expect(await Promise.race([offered.promise, unavailable.promise])).toBe(true);
      expect(await workspace.git.initialize(2)).toBe(true);
      expect(initialized).toEqual(["/moved"]);
    } finally {
      controller.abort();
    }
  },
);
