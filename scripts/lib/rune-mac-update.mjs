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
  await NodeFSP.writeFile(
    NodePath.join(directory, name),
    serializeUpdateManifest(
      {
        version,
        releaseDate: new Date().toISOString(),
        files,
        extras: { path: zip, sha512: files[0].sha512 },
      },
      { platformLabel: "macOS" },
    ),
  );
  return [...names, name];
}
