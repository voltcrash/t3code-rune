import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it, vi } from "vitest";
import { downloadRelease, InvalidReleaseFile } from "./rune-release-file.mjs";
import { installedLinuxPackage, installLinuxPackage } from "./rune-linux-update.mjs";
import { runeUpdates } from "./rune-updates.mjs";

it("publishes only complete downloads with the offered digest and preserves a previous installer on failure", async () => {
  await using cache = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-update-"),
  );
  const bytes = "verified";
  const release = {
    version: "1.3.0",
    filename: "T3-Code-1.3.0-arm64.zip",
    url: "https://mirror.example/update",
    size: bytes.length,
    sha512: NodeCrypto.createHash("sha512").update(bytes).digest("base64"),
  };
  using serve = vi.spyOn(globalThis, "fetch").mockImplementation(async () => new Response(bytes));
  const signal = new AbortController().signal;
  const destination = await downloadRelease(release, cache.path, signal);
  for (const payload of ["invalid!", "short", "too many bytes"]) {
    serve.mockImplementationOnce(async () => new Response(payload));
    await expect(downloadRelease(release, cache.path, signal)).rejects.toThrow();
    expect(await NodeFSP.readFile(NodePath.join(destination, release.filename), "utf8")).toBe(
      bytes,
    );
    expect(await NodeFSP.readdir(cache.path)).toEqual(["1.3.0"]);
  }
  const invalid = NodePath.join(destination, release.filename);
  await NodeFSP.unlink(invalid);
  await NodeFSP.mkdir(invalid);
  const recovered = await downloadRelease(release, cache.path, signal);
  expect(await NodeFSP.readFile(NodePath.join(recovered, release.filename), "utf8")).toBe(bytes);
});

it("rechecks the administrator's feed and update permission before downloading a displayed offer", async () => {
  await using resources = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-release-"),
  );
  await NodeFSP.writeFile(
    NodePath.join(resources.path, "rune-release.json"),
    JSON.stringify({ repository: null, arch: "arm64", format: "exe" }),
  );
  const filename = "T3-Code-1.3.0-arm64.exe";
  const sha512 = Buffer.alloc(64, 1).toString("base64");
  using serve = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(
      async () =>
        new Response(
          JSON.stringify({ version: "1.3.0", files: [{ url: filename, size: 42, sha512 }] }),
        ),
    );
  const publish = vi.fn();
  let policy = {
    mode: "auto",
    feedUrl: "https://original.example",
    allowNightly: true,
    message: null,
  };
  const request = runeUpdates({
    current: "1.2.0",
    resources: resources.path,
    cache: resources.path,
    signal: new AbortController().signal,
    publish,
    readPolicy: async () => policy,
  });
  await request("checkupdates");
  policy = { ...policy, feedUrl: "https://replacement.example" };
  await request("downloadupdate");
  expect(serve.mock.calls.map(([url]) => url)).toEqual([
    "https://original.example/latest.yml",
    "https://replacement.example/latest.yml",
  ]);
  expect(publish.mock.lastCall[0].available).toBe(true);
  policy = { ...policy, mode: "off" };
  await request("downloadupdate");
  expect(serve).toHaveBeenCalledTimes(2);
  expect(publish.mock.lastCall[0].title).toBe("Updates unavailable");
});

it.runIf(NodePath.sep === "/")(
  "updates the launched AppImage through a symlink and leaves externally replaced or corrupt images untouched",
  async () => {
    await using tmp = await NodeFSP.mkdtempDisposable(
      NodePath.join(NodeOS.tmpdir(), "t3-installed-image-"),
    );
    const root = NodePath.join(tmp.path, "mount");
    const resources = NodePath.join(root, "opt/T3 Code (Beta)/resources");
    await NodeFSP.mkdir(resources, { recursive: true });
    const target = NodePath.join(tmp.path, "T3.AppImage");
    const link = NodePath.join(tmp.path, "linked.AppImage");
    const cache = NodePath.join(tmp.path, "download");
    await NodeFSP.writeFile(target, "old", { mode: 0o755 });
    await NodeFSP.chmod(target, 0o755);
    await NodeFSP.symlink(target, link);
    const installed = await installedLinuxPackage(
      resources,
      { APPDIR: root, APPIMAGE: link },
      1000,
    );
    const release = {
      filename: "T3-Code-arm64.AppImage",
      size: 3,
      sha512: NodeCrypto.createHash("sha512").update("new").digest("base64"),
    };
    const signal = new AbortController().signal;
    await expect(installLinuxPackage(installed, release, cache, signal)).rejects.toBeInstanceOf(
      InvalidReleaseFile,
    );
    await NodeFSP.mkdir(cache);
    await expect(installLinuxPackage(installed, release, cache, signal)).rejects.toBeInstanceOf(
      InvalidReleaseFile,
    );
    await NodeFSP.rmdir(cache);
    await NodeFSP.writeFile(cache, "bad");
    await expect(installLinuxPackage(installed, release, cache, signal)).rejects.toBeInstanceOf(
      InvalidReleaseFile,
    );
    expect(await NodeFSP.readFile(target, "utf8")).toBe("old");
    await NodeFSP.writeFile(cache, "new");
    await expect(installLinuxPackage(installed, release, cache, signal)).resolves.toBe(
      await NodeFSP.realpath(target),
    );
    expect(await NodeFSP.readFile(link, "utf8")).toBe("new");
    expect((await NodeFSP.stat(target)).mode & 0o777).toBe(0o755);
    expect((await NodeFSP.lstat(link)).isSymbolicLink()).toBe(true);
    await expect(installLinuxPackage(installed, release, cache, signal)).rejects.toThrow(
      "has changed",
    );
  },
);
