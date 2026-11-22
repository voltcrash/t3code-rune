import { WS_METHODS } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { expect, it, vi } from "vite-plus/test";
import { workspaceEditors } from "./rune-editors.mjs";
import { findRemoteEditors } from "../src/app/remoteEditors.ts";
import { openExternalUrl } from "./rune-external.mjs";

vi.mock("./rune-external.mjs", () => ({ openExternalUrl: vi.fn(async () => true) }));
vi.mock("../src/app/remoteEditors.ts", () => ({
  findRemoteEditors: vi.fn(() => Effect.succeed(["vscode"])),
}));

function workspace(local, canOperate, publish = vi.fn()) {
  const controller = new AbortController();
  const save = vi.fn();
  const editors = workspaceEditors({
    local,
    session: { platform: "linux", label: "Remote workspace", canOperate },
    signal: controller.signal,
    preferred: "vscode",
    sshHintSeen: false,
    markSshHintSeen: vi.fn(),
    save,
    publish,
  });
  return { editors, save, publish, [Symbol.dispose]: () => controller.abort() };
}

it("keeps local execution within the current connection and its operate grant", async () => {
  const open = vi.fn(() => Effect.void);
  const rpc = { [WS_METHODS.shellOpenInEditor]: open };
  const config = { availableEditors: ["vscode", "cursor"] };
  const cwd = "/work/repo:42";
  using allowed = workspace(true, true);
  using denied = workspace(true, false);
  for (const owner of [allowed, denied]) owner.editors.configure(config, rpc);
  expect(allowed.editors.choose("cursor")).toBe(true);
  expect(allowed.save).toHaveBeenCalledWith("cursor");
  expect(open).not.toHaveBeenCalled();
  expect(await allowed.editors.open("cursor", cwd)).toBe(true);
  expect(open).toHaveBeenCalledWith({ editor: "cursor", cwd, position: {} });
  expect(await denied.editors.open("vscode", cwd)).toBe(false);
  allowed.editors.configure(undefined, undefined);
  expect(await allowed.editors.open("cursor", cwd)).toBe(false);
  expect(open).toHaveBeenCalledTimes(1);
});

it("opens a paired workspace through the viewing machine using its current SSH destination", async () => {
  const open = vi.fn();
  const rpc = { [WS_METHODS.shellOpenInEditor]: open };
  const completeProbe = Promise.withResolvers();
  findRemoteEditors.mockReturnValueOnce(Effect.promise(() => completeProbe.promise));
  const emptyPublished = Promise.withResolvers();
  using empty = workspace(false, false, (catalog) => {
    if (catalog.canChoose) emptyPublished.resolve(catalog);
  });
  const route = {
    availableEditors: ["file-manager"],
    remoteOpenTargets: [{ kind: "mdns", host: "dev.local" }],
  };
  empty.editors.configure(route, rpc);
  expect(await empty.editors.open("vscode", "/repo")).toBe(false);
  completeProbe.resolve([]);
  expect(await emptyPublished.promise).toMatchObject({ editors: [], canOpen: false });
  expect(await empty.editors.open("vscode", "/repo")).toBe(false);
  const found = Promise.withResolvers();
  using owner = workspace(false, false, (catalog) => {
    if (catalog.canChoose) found.resolve(catalog);
  });
  expect(await found.promise).toMatchObject({ preferred: "vscode" });
  const config = { availableEditors: ["file-manager"], remoteOpenTargets: [] };
  owner.editors.configure(config, rpc);
  expect(await owner.editors.open("vscode", "/repo")).toBe(false);
  owner.editors.configure(
    { ...config, remoteOpenTargets: [{ kind: "mdns", host: "dev.local" }] },
    rpc,
  );
  expect(await owner.editors.open("file-manager", "/repo")).toBe(false);
  expect(await owner.editors.open("vscode", "/work/project name")).toBe(true);
  expect(owner.editors.catalog().hint).toBeUndefined();
  expect(openExternalUrl).toHaveBeenCalledWith(
    "vscode://vscode-remote/ssh-remote+dev.local/work/project%20name",
  );
  expect(open).not.toHaveBeenCalled();
});
