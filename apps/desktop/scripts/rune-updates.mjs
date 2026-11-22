import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Effect from "effect/Effect";
import * as Logger from "effect/Logger";
import * as DesktopManagedPolicy from "../src/app/DesktopManagedPolicy.ts";
import * as DesktopUpdatePolicy from "../src/updates/DesktopUpdatePolicy.ts";
import { downloadRelease, InvalidReleaseFile } from "./rune-release-file.mjs";
import { installedLinuxPackage, installLinuxPackage } from "./rune-linux-update.mjs";
import { installedMacPackage, installMacPackage } from "./rune-mac-update.mjs";
import { configuredDisableAutoUpdate } from "../src/app/DesktopConfig.ts";
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
  const platform = Effect.runSync(HostProcess.Platform);
  const installed = (async () => {
    if (!resources) return;
    if (platform === "darwin") return installedMacPackage(resources);
    if (platform === "linux")
      return installedLinuxPackage(
        resources,
        Effect.runSync(HostProcess.Environment),
        Effect.runSync(HostProcess.UserId),
      );
  })().catch(() => undefined);
  let downloaded;
  let release;
  let directory;
  let busy = false;
  const report = (title, message, fields) =>
    publish({ type: "releasecheck", title, message, ...fields });
  return async (request) => {
    if (request === "showupdate") {
      if (directory) {
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
    const install = request === "installupdate";
    if (busy) return;
    busy = true;
    try {
      if (await Effect.runPromise(configuredDisableAutoUpdate)) {
        report(
          "Updates unavailable",
          "Automatic updates are disabled by the T3CODE_DISABLE_AUTO_UPDATE setting.",
        );
        return;
      }
      const previous = install ? downloaded : release;
      release = undefined;
      const source = await readSource(resources);
      const policy = await readPolicy(signal);
      const timeout = AbortSignal.any([signal, AbortSignal.timeout(30_000)]);
      release = await checkRuneRelease(current, source, policy, (url) => releaseText(url, timeout));
      if (
        (download || install) &&
        previous?.status === "available" &&
        release.status === "available" &&
        previous.version === release.version &&
        previous.url === release.url &&
        previous.sha512 === release.sha512 &&
        previous.size === release.size
      ) {
        const target = await installed;
        if (install && directory && target?.format === source?.format) {
          const apply = target.format === "zip" ? installMacPackage : installLinuxPackage;
          const executable = await apply(
            target,
            release,
            NodePath.join(directory, release.filename),
            signal,
          );
          publish({ type: "restartupdated", executable });
          return;
        }
        directory = await downloadRelease(
          release,
          cache,
          AbortSignal.any([signal, AbortSignal.timeout(20 * 60_000)]),
        );
        downloaded = release;
        report(
          "Update downloaded",
          target?.format === source?.format
            ? "T3 Code will restart to install it. Any running tasks will be interrupted."
            : release.filename,
          { downloaded: true, installable: target?.format === source?.format },
        );
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
    } catch (error) {
      if (install && error instanceof InvalidReleaseFile) {
        directory = undefined;
        downloaded = undefined;
        report(
          "Download the update again",
          "The cached file is missing or did not match the release.",
          { available: true },
        );
      } else if (install)
        report("Update could not be installed", error.message, {
          downloaded: true,
          installable: true,
        });
      else if (download)
        report("Download failed", "Could not download and verify the update.", {
          available: release?.status === "available",
        });
      else report("Update check failed", "Could not check for updates. Please try again later.");
    } finally {
      busy = false;
    }
  };
}
