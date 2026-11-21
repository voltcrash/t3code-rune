import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeStreamPromises from "node:stream/promises";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Effect from "effect/Effect";
import * as Logger from "effect/Logger";
import * as DesktopManagedPolicy from "../src/app/DesktopManagedPolicy.ts";
import * as DesktopUpdatePolicy from "../src/updates/DesktopUpdatePolicy.ts";
import { checkRuneRelease, decodeReleaseSource } from "../src/native/releases.ts";

async function readSource(directory) {
  if (!directory) return;
  try {
    return decodeReleaseSource(
      await NodeFSP.readFile(NodePath.join(directory, "rune-release.json"), "utf8"),
    );
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

async function releaseText(url, signal) {
  const response = await fetch(url, { signal, headers: { "User-Agent": "T3-Code-Desktop" } });
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    if (response.status === 404) return;
    throw new Error("Could not read the release feed.");
  }
  const chunks = [];
  let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.byteLength;
    if (bytes > 16 * 1024 * 1024) throw new Error("The release feed is too large.");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
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
  const hash = NodeCrypto.createHash("sha512");
  let size = 0;
  await NodeStreamPromises.pipeline(
    response.body,
    async function* (bytes) {
      for await (const chunk of bytes) {
        size += chunk.length;
        if (size > release.size) throw new Error("Invalid download size.");
        hash.update(chunk);
        yield chunk;
      }
    },
    NodeFS.createWriteStream(file, { flags: "wx", mode: 0o700 }),
    { signal },
  );
  if (size !== release.size || hash.digest("base64") !== release.sha512) {
    throw new Error("The download did not match the release.");
  }
  // AppImages keep the same filename across releases.
  const destination = NodePath.join(cache, release.version);
  await NodeFSP.mkdir(destination, { recursive: true });
  await NodeFSP.rename(file, NodePath.join(destination, release.filename));
  return destination;
}

const readUpdatePolicy = (signal) =>
  Effect.runPromise(
    Effect.service(DesktopUpdatePolicy.DesktopUpdatePolicy).pipe(
      Effect.provide(DesktopUpdatePolicy.layer),
      Effect.provide(DesktopManagedPolicy.layer),
      Effect.provide(NodeServices.layer),
      Effect.provideService(Logger.LogToStderr, true),
    ),
    { signal },
  );

export function runeUpdates({
  current,
  resources,
  cache,
  signal,
  publish,
  readPolicy = readUpdatePolicy,
}) {
  let release;
  let directory;
  let busy = false;
  const report = (title, message, fields) =>
    publish({ type: "releasecheck", title, message, ...fields });
  return async (request) => {
    if (request === "showupdate") {
      if (directory) {
        const platform = await Effect.runPromise(HostProcess.Platform);
        const open =
          platform === "darwin"
            ? "/usr/bin/open"
            : platform === "win32"
              ? "explorer.exe"
              : "xdg-open";
        const child = NodeChildProcess.spawn(open, [directory], {
          stdio: "ignore",
          windowsHide: true,
        });
        child.once("error", () => report("Update downloaded", directory, { downloaded: true }));
        child.unref();
      }
      return;
    }
    const download = request === "downloadupdate";
    if (busy) return;
    busy = true;
    try {
      const previous = release;
      release = undefined;
      const source = await readSource(resources);
      const policy = await readPolicy(signal);
      const timeout = AbortSignal.any([signal, AbortSignal.timeout(30_000)]);
      release = await checkRuneRelease(current, source, policy, (url) => releaseText(url, timeout));
      if (
        download &&
        previous?.status === "available" &&
        release.status === "available" &&
        previous.version === release.version &&
        previous.url === release.url &&
        previous.sha512 === release.sha512 &&
        previous.size === release.size
      ) {
        directory = await downloadRelease(
          release,
          cache,
          AbortSignal.any([signal, AbortSignal.timeout(20 * 60_000)]),
        );
        report("Update downloaded", release.filename, { downloaded: true });
        return;
      }
      switch (release.status) {
        case "available":
          report(
            "Update available",
            `T3 Code ${release.version} is available. You’re running T3 Code ${current}.`,
            { available: true },
          );
          break;
        case "current":
          report(
            "You’re up to date!",
            `T3 Code ${current} is currently the newest version available.`,
          );
          break;
        case "disabled":
          report("Updates unavailable", release.message);
      }
    } catch {
      if (download)
        report("Download failed", "Could not download and verify the update.", {
          available: release?.status === "available",
        });
      else report("Update check failed", "Could not check for updates. Please try again later.");
    } finally {
      busy = false;
    }
  };
}
