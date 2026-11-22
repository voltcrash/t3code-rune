import * as NodeChildProcess from "node:child_process";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeUtil from "node:util";
import { lock } from "proper-lockfile";
import { stageCachedRelease } from "./rune-release-file.mjs";

const execFile = NodeUtil.promisify(NodeChildProcess.execFile);
const launcher = (app) => NodePath.join(app, "Contents/MacOS/t3-code");

export async function installedMacPackage(resources) {
  const directory = await NodeFSP.realpath(resources);
  if (!directory.endsWith(".app/Contents/Resources") || directory.includes("/AppTranslocation/"))
    return;
  const app = NodePath.dirname(NodePath.dirname(directory));
  await NodeFSP.access(NodePath.dirname(app), NodeFS.constants.W_OK);
  const { dev, ino } = await NodeFSP.stat(app, { bigint: true });
  return { format: "zip", app, dev, ino };
}

async function bundleField(app, field) {
  const plist = NodePath.join(app, "Contents/Info.plist");
  const { stdout } = await execFile("/usr/bin/plutil", [
    "-extract",
    field,
    "raw",
    "-o",
    "-",
    plist,
  ]);
  return stdout.trim();
}

async function verifyMacRelease(installed, app, release) {
  const [originalId, id, version, executable] = await Promise.all([
    bundleField(installed, "CFBundleIdentifier"),
    bundleField(app, "CFBundleIdentifier"),
    bundleField(app, "CFBundleShortVersionString"),
    bundleField(app, "CFBundleExecutable"),
  ]);
  if (id !== originalId || version !== release.version || executable !== "t3-code") {
    throw new Error("The downloaded app does not match this installation or release.");
  }
  const { stdout, stderr } = await execFile("/usr/bin/codesign", [
    "--display",
    "-r-",
    "--verbose=3",
    installed,
  ]);
  const args = ["--verify", "--deep", "--strict", "--all-architectures"];
  if (!/^Signature=adhoc$/m.test(stderr)) {
    const requirement = /^designated => (.+)$/m.exec(stdout)?.[1];
    if (!requirement) throw new Error("Could not read the installed app’s signing requirement.");
    args.push("--test-requirement", "=" + requirement);
  }
  await execFile("/usr/bin/codesign", [...args, app]);
  await Promise.all([
    NodeFSP.access(launcher(app), NodeFS.constants.X_OK),
    NodeFSP.access(NodePath.join(app, "Contents/Helpers/node"), NodeFS.constants.X_OK),
    NodeFSP.access(
      NodePath.join(app, "Contents/Resources/rune-projects.mjs"),
      NodeFS.constants.R_OK,
    ),
  ]);
}

export async function installMacPackage(installed, release, cached, signal) {
  const parent = NodePath.dirname(installed.app);
  await using cleanup = new AsyncDisposableStack();
  const stage = await NodeFSP.mkdtempDisposable(NodePath.join(parent, ".t3-update-"));
  cleanup.defer(() =>
    stage
      .remove()
      .catch((error) => console.warn("Could not remove update staging directory", error)),
  );
  const archive = NodePath.join(stage.path, release.filename);
  await stageCachedRelease(cached, archive, release, signal);
  const extracted = NodePath.join(stage.path, "unpacked");
  await NodeFSP.mkdir(extracted);
  await execFile("/usr/bin/tar", ["-xf", archive, "-C", extracted], { signal });
  const entries = (await NodeFSP.readdir(extracted, { withFileTypes: true })).filter(
    (e) => e.isDirectory() && e.name.endsWith(".app"),
  );
  if (entries.length !== 1) throw new Error("The update must contain one application bundle.");
  const app = NodePath.join(extracted, entries[0].name);
  const unlock = await lock(installed.app, { realpath: false });
  cleanup.defer(() =>
    unlock().catch((error) => console.warn("Could not release update lock", error)),
  );
  const original = await NodeFSP.stat(installed.app, { bigint: true });
  if (original.dev !== installed.dev || original.ino !== installed.ino)
    throw new Error("The installed app changed. Reopen it before updating.");
  await verifyMacRelease(installed.app, app, release);
  signal.throwIfAborted();
  // The kernel swaps both bundles in a single operation on their shared volume.
  await execFile(launcher(installed.app), [
    "--exchange-mac-update",
    app,
    String(original.dev),
    String(original.ino),
  ]);
  return launcher(installed.app);
}
