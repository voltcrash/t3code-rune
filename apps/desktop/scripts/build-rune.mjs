import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFS from "node:fs";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";

import { bundleRuneWorker } from "./bundle-rune-worker.mjs";

async function digestFile(file) {
  const hash = NodeCrypto.createHash("sha256");
  for await (const chunk of NodeFS.createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

const desktop = NodePath.resolve(import.meta.dirname, "..");
const native = NodePath.join(desktop, "native");
const target = NodeChildProcess.execFileSync("rustc", ["--print", "host-tuple"], {
  cwd: native,
  encoding: "utf8",
}).trim();
const executable = target.includes("-windows-") ? "t3-code.exe" : "t3-code";
NodeChildProcess.execFileSync(
  "cargo",
  [
    "build",
    "--release",
    "--locked",
    "--bin",
    "t3-code",
    "--target",
    target,
    "--target-dir",
    "target",
  ],
  { cwd: native, stdio: "inherit" },
);
const output = NodePath.join(desktop, "dist");
await NodeFSP.mkdir(output, { recursive: true });
const stage = await NodeFSP.mkdtemp(NodePath.join(output, ".rune-"));
try {
  const files = NodePath.join(stage, "files");
  await NodeFSP.mkdir(files);
  await NodeFSP.copyFile(
    NodePath.join(native, "target", target, "release", executable),
    NodePath.join(files, executable),
  );
  await bundleRuneWorker(files);
  const names = [executable, "rune-projects.mjs"];
  const hashes = await Promise.all(names.map((name) => digestFile(NodePath.join(files, name))));
  const generation = NodeCrypto.createHash("sha256").update(hashes.join("")).digest("hex");
  const destination = NodePath.join(output, generation);
  try {
    await NodeFSP.rename(files, destination);
  } catch (error) {
    if (!(await NodeFSP.stat(destination).catch(() => null))?.isDirectory()) throw error;
    for (const [i, name] of names.entries()) {
      const artifact = NodePath.join(destination, name);
      const mode = name === executable ? NodeFS.constants.X_OK : NodeFS.constants.R_OK;
      const existing = await NodeFSP.access(artifact, mode)
        .then(() => digestFile(artifact))
        .catch(() => null);
      if (existing !== hashes[i]) {
        await NodeFSP.rename(NodePath.join(files, name), artifact);
      }
    }
  }
  const pointer = NodePath.join(stage, "current");
  await NodeFSP.writeFile(pointer, generation + "\n");
  await NodeFSP.rename(pointer, NodePath.join(output, "current"));
} finally {
  await NodeFSP.rm(stage, { recursive: true, force: true });
}
