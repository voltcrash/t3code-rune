import * as NodePath from "node:path";
import { build } from "vite-plus/pack";

export async function bundleRuneWorker(outDir) {
  const desktop = NodePath.resolve(import.meta.dirname, "..");
  await build({
    cwd: desktop,
    config: false,
    entry: ["scripts/rune-projects.mjs"],
    format: "esm",
    platform: "node",
    target: `node${process.versions.node.split(".")[0]}`,
    dts: false,
    clean: false,
    shims: true,
    outDir,
    sourcemap: false,
    outputOptions: { codeSplitting: false },
    deps: {
      alwaysBundle: (id) =>
        !id.startsWith("node:") && !["bufferutil", "utf-8-validate"].includes(id),
      neverBundle: ["bufferutil", "utf-8-validate"],
      onlyBundle: false,
    },
  });
}
