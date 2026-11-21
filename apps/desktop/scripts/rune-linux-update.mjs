import * as NodeChildProcess from "node:child_process";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeUtil from "node:util";
import { InvalidReleaseFile, writeVerifiedRelease } from "./rune-release-file.mjs";
import { lock } from "proper-lockfile";

const execFile = NodeUtil.promisify(NodeChildProcess.execFile);

async function imageIdentity(target) {
  const stat = await NodeFSP.stat(target, { bigint: true });
  return {
    dev: stat.dev,
    ino: stat.ino,
    size: stat.size,
    modified: stat.mtimeNs,
    changed: stat.ctimeNs,
    mode: Number(stat.mode & 0o777n),
  };
}

export async function installedLinuxPackage(resources, env, uid) {
  if (
    env.APPIMAGE &&
    NodePath.isAbsolute(env.APPIMAGE) &&
    env.APPDIR &&
    NodePath.isAbsolute(env.APPDIR)
  ) {
    const root = await NodeFSP.realpath(env.APPDIR);
    const relative = NodePath.relative(root, await NodeFSP.realpath(resources));
    if (!/^opt\/[^/]+\/resources$/.test(relative)) return;
    const target = await NodeFSP.realpath(env.APPIMAGE);
    return { format: "AppImage", target, identity: await imageIdentity(target) };
  }
  const launcher = NodePath.resolve(resources, "../t3code");
  if (!/^\/opt\/[^/]+\/t3code$/.test(launcher)) return;
  const { stdout } = await execFile("/usr/bin/dpkg-query", ["--search", launcher]);
  if (stdout.trim() !== `t3code: ${launcher}`) return;
  const elevated = uid !== 0;
  if (elevated) await NodeFSP.access("/usr/bin/pkexec", NodeFS.constants.X_OK);
  return { format: "deb", target: launcher, elevated };
}

async function installDebianPackage(installed, replacement) {
  const apt = "/usr/bin/apt-get";
  const args = ["--yes", "--no-remove", "--no-install-recommends", "install", replacement];
  const child = NodeChildProcess.spawn(
    installed.elevated ? "/usr/bin/pkexec" : apt,
    installed.elevated ? [apt, ...args] : args,
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  const diagnostics = { stdout: "", stderr: "" };
  for (const name of ["stdout", "stderr"]) {
    child[name].setEncoding("utf8").on("data", (chunk) => {
      diagnostics[name] = (diagnostics[name] + chunk).slice(-1024);
    });
  }
  await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => {
      if (code === 0) return resolve();
      const reason =
        installed.elevated && (code === 126 || code === 127)
          ? "Administrator authorization was not granted."
          : [diagnostics.stdout.trim(), diagnostics.stderr.trim()].filter(Boolean).join("\n") ||
            `APT exited with ${signal ?? code}.`;
      reject(new Error(reason));
    });
  });
}

async function* readCachedRelease(cached) {
  try {
    await using file = await NodeFSP.open(
      cached,
      NodeFS.constants.O_RDONLY | NodeFS.constants.O_NONBLOCK,
    );
    if (!(await file.stat()).isFile()) throw new Error("Expected a regular file.");
    yield* file.createReadStream({ autoClose: false });
  } catch (cause) {
    throw new InvalidReleaseFile("The downloaded file cannot be read.", { cause });
  }
}

export async function installLinuxPackage(installed, release, cached, signal) {
  const image = installed.format === "AppImage";
  const parent = image ? NodePath.dirname(installed.target) : NodeOS.tmpdir();
  await using stage = await NodeFSP.mkdtempDisposable(NodePath.join(parent, ".t3-update-"));
  const replacement = NodePath.join(stage.path, release.filename);
  await writeVerifiedRelease(readCachedRelease(cached), replacement, release, signal);
  if (image) {
    const unlock = await lock(installed.target, { realpath: false });
    try {
      const current = await imageIdentity(installed.target);
      if (!NodeUtil.isDeepStrictEqual(current, installed.identity)) {
        throw new Error("The installed AppImage has changed. Reopen the app to update it.");
      }
      await NodeFSP.chmod(replacement, current.mode);
      await using contents = await NodeFSP.open(replacement);
      await contents.sync();
      signal.throwIfAborted();
      await NodeFSP.rename(replacement, installed.target);
    } finally {
      await unlock();
    }
  } else {
    // Once apt starts a transaction, let dpkg finish even if the desktop closes.
    signal.throwIfAborted();
    await installDebianPackage(installed, replacement);
  }
  return installed.target;
}
