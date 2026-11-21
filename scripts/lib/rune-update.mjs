import * as NodeCrypto from "node:crypto";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeStreamPromises from "node:stream/promises";
import { BlockMap } from "better-blockmap";
import { serializeUpdateManifest } from "./update-manifest.ts";

export async function writeMacUpdateFiles(directory, { version, channel, arch, zip, dmg }) {
  const files = [];
  const names = [];
  for (const archive of dmg ? [zip, dmg] : [zip]) {
    const map = new BlockMap({ detectZipBoundary: archive === zip });
    await NodeStreamPromises.pipeline(
      NodeFS.createReadStream(NodePath.join(directory, archive)),
      map,
    );
    const name = archive + ".blockmap";
    await NodeFSP.writeFile(NodePath.join(directory, name), map.compress("gzip"));
    files.push({ url: archive, size: map.stats.size, sha512: map.stats.sha512 });
    names.push(name);
  }
  const name = channel + (arch === "x64" ? "-mac-x64.yml" : "-mac.yml");
  await writeManifest(directory, name, version, files, "macOS");
  return [...names, name];
}

async function writeManifest(directory, name, version, files, platformLabel) {
  await NodeFSP.writeFile(
    NodePath.join(directory, name),
    serializeUpdateManifest(
      {
        version,
        releaseDate: new Date().toISOString(),
        files,
        extras: { path: files[0].url, sha512: files[0].sha512 },
      },
      { platformLabel },
    ),
  );
}

export async function writeLinuxUpdateFiles(directory, { version, channel, arch, image, deb }) {
  const path = NodePath.join(directory, image);
  const map = new BlockMap();
  const hash = NodeCrypto.createHash("sha512");
  const stream = NodeFS.createReadStream(path);
  stream.on("data", (chunk) => hash.update(chunk));
  await NodeStreamPromises.pipeline(stream, map);
  const blockmap = map.compress("deflate");
  const length = Buffer.alloc(4);
  length.writeUInt32BE(blockmap.length);
  const trailer = Buffer.concat([blockmap, length]);
  await NodeFSP.appendFile(path, trailer);
  const files = [
    {
      url: image,
      size: map.stats.size + trailer.length,
      blockMapSize: blockmap.length,
      sha512: hash.update(trailer).digest("base64"),
    },
  ];
  const debPath = NodePath.join(directory, deb);
  const debHash = NodeCrypto.createHash("sha512");
  for await (const chunk of NodeFS.createReadStream(debPath)) debHash.update(chunk);
  files.push({
    url: deb,
    size: (await NodeFSP.stat(debPath)).size,
    sha512: debHash.digest("base64"),
  });
  const name = channel + (arch === "x64" ? "-linux.yml" : "-linux-arm64.yml");
  await writeManifest(directory, name, version, files, "Linux");
  return [name];
}
