import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeZlib from "node:zlib";
import { expect, it } from "vitest";
import {
  writeLinuxUpdateFiles,
  writeMacUpdateFiles,
  writeWindowsUpdateFiles,
} from "./rune-update.mjs";
import { parseUpdateManifest } from "./update-manifest.ts";

it("stages both Windows installers and their external block maps for release merging", async () => {
  await using dir = await NodeFSP.mkdtempDisposable(
    NodePath.join(NodeOS.tmpdir(), "rune-win-update-"),
  );
  const version = "1.2.3-nightly.20261121.42";
  for (const arch of ["x64", "arm64"]) {
    const installer = `T3-Code-${version}-${arch}.exe`;
    const bytes = Buffer.alloc(96_000, arch === "arm64" ? 1 : 2);
    await NodeFSP.writeFile(NodePath.join(dir.path, installer), bytes);
    const name = `nightly-win-${arch}.yml`;
    expect(
      await writeWindowsUpdateFiles(dir.path, { version, channel: "nightly", arch, installer }),
    ).toEqual([installer + ".blockmap", name]);
    const raw = await NodeFSP.readFile(NodePath.join(dir.path, name), "utf8");
    expect(parseUpdateManifest(raw, name, "Windows").files).toEqual([
      { url: installer, size: bytes.length, sha512: NodeCrypto.hash("sha512", bytes, "base64") },
    ]);
    const map = JSON.parse(
      NodeZlib.gunzipSync(await NodeFSP.readFile(NodePath.join(dir.path, installer + ".blockmap"))),
    );
    expect(map.version).toBe("2");
    expect(map.files[0].sizes.reduce((total, size) => total + size, 0)).toBe(bytes.length);
  }
});

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

it.each([
  ["arm64", "nightly-linux-arm64.yml"],
  ["x64", "nightly-linux.yml"],
])(
  "embeds a range-readable block map and hashes the complete Linux release for %s",
  async (arch, name) => {
    await using dir = await NodeFSP.mkdtempDisposable(
      NodePath.join(NodeOS.tmpdir(), "rune-linux-update-"),
    );
    const image = "test.AppImage",
      deb = "test.deb";
    const payload = NodeCrypto.randomBytes(128_000);
    const debBytes = Buffer.from("debian package");
    await NodeFSP.writeFile(NodePath.join(dir.path, image), payload);
    await NodeFSP.writeFile(NodePath.join(dir.path, deb), debBytes);
    expect(
      await writeLinuxUpdateFiles(dir.path, {
        version: "1.2.3-nightly.20261121.1",
        channel: "nightly",
        arch,
        image,
        deb,
      }),
    ).toEqual([name]);
    const bytes = await NodeFSP.readFile(NodePath.join(dir.path, image));
    const blockMapSize = bytes.readUInt32BE(bytes.length - 4);
    expect(bytes.subarray(0, payload.length)).toEqual(payload);
    expect(payload.length + blockMapSize + 4).toBe(bytes.length);
    const [map] = JSON.parse(NodeZlib.inflateRawSync(bytes.subarray(payload.length, -4))).files;
    expect(map.sizes.reduce((total, size) => total + size, 0)).toBe(payload.length);
    expect(map.checksums).toHaveLength(map.sizes.length);
    expect(
      parseUpdateManifest(
        await NodeFSP.readFile(NodePath.join(dir.path, name), "utf8"),
        name,
        "Linux",
      ).files,
    ).toEqual([
      {
        url: image,
        size: bytes.length,
        sha512: NodeCrypto.hash("sha512", bytes, "base64"),
        blockMapSize,
      },
      { url: deb, size: debBytes.length, sha512: NodeCrypto.hash("sha512", debBytes, "base64") },
    ]);
  },
);
