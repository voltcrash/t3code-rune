import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeZlib from "node:zlib";
import { expect, it } from "vitest";
import { writeMacUpdateFiles } from "./rune-mac-update.mjs";
import { parseUpdateManifest } from "./update-manifest.ts";

it("describes the final ZIP and disk image bytes in the macOS update feed", async () => {
  await using dir = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-mac-update-"),
  );
  const zip = "T3-Code-1.2.3-arm64.zip";
  const dmg = "T3-Code-1.2.3-arm64.dmg";
  const expected = [];
  for (const [name, bytes] of [
    [zip, Buffer.alloc(96_000, 13)],
    [dmg, Buffer.alloc(64_000, 23)],
  ]) {
    await NodeFSP.writeFile(NodePath.join(dir.path, name), bytes);
    expected.push({
      url: name,
      size: bytes.length,
      sha512: NodeCrypto.createHash("sha512").update(bytes).digest("base64"),
    });
  }
  const outputs = await writeMacUpdateFiles(dir.path, {
    version: "1.2.3",
    channel: "latest",
    arch: "arm64",
    zip,
    dmg,
  });
  expect(outputs).toEqual([zip + ".blockmap", dmg + ".blockmap", "latest-mac.yml"]);
  const raw = await NodeFSP.readFile(NodePath.join(dir.path, "latest-mac.yml"), "utf8");
  expect(parseUpdateManifest(raw, "latest-mac.yml", "macOS").files).toEqual(expected);
  for (const file of expected) {
    const map = JSON.parse(
      NodeZlib.gunzipSync(await NodeFSP.readFile(NodePath.join(dir.path, file.url + ".blockmap"))),
    );
    expect(map.version).toBe("2");
    const [entry] = map.files;
    expect(entry.checksums.length).toBe(entry.sizes.length);
    expect(entry.sizes.reduce((total, size) => total + size, 0)).toBe(file.size);
  }
  const intel = "T3-Code-1.2.3-x64.zip";
  await NodeFSP.copyFile(NodePath.join(dir.path, zip), NodePath.join(dir.path, intel));
  expect(
    await writeMacUpdateFiles(dir.path, {
      version: "1.2.3",
      channel: "latest",
      arch: "x64",
      zip: intel,
    }),
  ).toEqual([intel + ".blockmap", "latest-mac-x64.yml"]);
  expect(await NodeFSP.readFile(NodePath.join(dir.path, "latest-mac.yml"), "utf8")).toBe(raw);
});
