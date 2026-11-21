import * as NodeCrypto from "node:crypto";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeStreamPromises from "node:stream/promises";

export class InvalidReleaseFile extends Error {}

export async function writeVerifiedRelease(body, file, release, signal) {
  const hash = NodeCrypto.createHash("sha512");
  let size = 0;
  await NodeStreamPromises.pipeline(
    body,
    async function* (bytes) {
      for await (const chunk of bytes) {
        size += chunk.length;
        if (size > release.size) throw new InvalidReleaseFile("Invalid download size.");
        hash.update(chunk);
        yield chunk;
      }
    },
    NodeFS.createWriteStream(file, { flags: "wx", mode: 0o700 }),
    { signal },
  );
  if (size !== release.size || hash.digest("base64") !== release.sha512) {
    throw new InvalidReleaseFile("The download did not match the release.");
  }
}

export async function downloadRelease(release, cache, signal) {
  await NodeFSP.mkdir(cache, { recursive: true, mode: 0o700 });
  await using stage = await NodeFSP.mkdtempDisposable(NodePath.join(cache, ".download-"));
  const file = NodePath.join(stage.path, release.filename);
  const response = await fetch(release.url, { signal });
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw new Error("Could not download the update.");
  }
  await writeVerifiedRelease(response.body, file, release, signal);
  // AppImages keep the same filename across releases.
  const destination = NodePath.join(cache, release.version);
  await NodeFSP.mkdir(destination, { recursive: true });
  const output = NodePath.join(destination, release.filename);
  try {
    await NodeFSP.rename(file, output);
    return destination;
  } catch (error) {
    if (!(await NodeFSP.lstat(output).catch(() => undefined))?.isDirectory()) throw error;
    // Preserve unexpected directory contents and publish to a new cache location.
    const recovered = await NodeFSP.mkdtemp(NodePath.join(cache, release.version + "-"));
    try {
      await NodeFSP.rename(file, NodePath.join(recovered, release.filename));
    } catch (cause) {
      await NodeFSP.rmdir(recovered);
      throw cause;
    }
    return recovered;
  }
}
