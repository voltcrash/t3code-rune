import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it, vi } from "vitest";
import { downloadRelease, runeUpdates } from "./rune-updates.mjs";

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
