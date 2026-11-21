import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import { build } from "vite-plus/pack";
import { resolveTaggedSourceVersion } from "../../../scripts/lib/tagged-source-version.ts";

const desktop = NodePath.resolve(import.meta.dirname, "..");
const bundleDir = NodePath.join(desktop, "dist/rune-bundle");
const workerName = "rune-projects.mjs";
async function bundleHeader(version, serverDist) {
  // The entry points reference content-hashed chunks, including the versioned server and client.
  const hash = NodeCrypto.createHash("sha256");
  for (const file of [
    "bin.mjs",
    "claude-history-worker.mjs",
    "client/index.html",
    "client/asset-manifest.json",
  ]) {
    hash.update(await NodeFSP.readFile(NodePath.join(serverDist, file)));
  }
  return `// T3 Code Rune worker ${JSON.stringify(version)} ${hash.digest("hex")}`;
}

export async function readPrebuiltRuneWorker(version, serverDist) {
  const bytes = await NodeFSP.readFile(NodePath.join(bundleDir, workerName));
  const header = Buffer.from((await bundleHeader(version, serverDist)) + "\n");
  if (!bytes.subarray(0, header.length).equals(header) || bytes.length === header.length) {
    throw new Error(`Rebuild the desktop JS bundle for version ${version}.`);
  }
  return bytes;
}

export async function bundleRuneWorker(outDir, version, banner) {
  await build({
    cwd: desktop,
    config: false,
    ...(version ? { define: { "import.meta.t3codeVersion": JSON.stringify(version) } } : {}),
    entry: ["scripts/rune-projects.mjs"],
    format: "esm",
    platform: "node",
    target: `node${process.versions.node.split(".")[0]}`,
    dts: false,
    clean: false,
    shims: true,
    outDir,
    sourcemap: false,
    outputOptions: { codeSplitting: false, ...(banner && { banner }) },
    deps: {
      alwaysBundle: (id) =>
        !id.startsWith("node:") && !["bufferutil", "utf-8-validate", "ffi-rs"].includes(id),
      neverBundle: ["bufferutil", "utf-8-validate", "ffi-rs"],
      onlyBundle: false,
    },
  });
}

if (import.meta.main) {
  const version =
    process.env.APP_VERSION?.trim() ||
    resolveTaggedSourceVersion(
      NodePath.resolve(desktop, "../.."),
      JSON.parse(await NodeFSP.readFile(NodePath.join(desktop, "package.json"), "utf8")).version,
    );
  await NodeFSP.mkdir(bundleDir, { recursive: true });
  await using stage = await NodeFSP.mkdtempDisposable(NodePath.join(bundleDir, ".build-"));
  await bundleRuneWorker(
    stage.path,
    version,
    await bundleHeader(version, NodePath.resolve(desktop, "../server/dist")),
  );
  await NodeFSP.rename(NodePath.join(stage.path, workerName), NodePath.join(bundleDir, workerName));
}
