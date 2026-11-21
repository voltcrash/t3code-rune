import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";

const checksums = {
  "26.13.1": {
    arm64: "aec2514ea1ceb2ba52455e9ec65986f6749a06dd759e5db2a72e6889668c9735",
    x64: "82bb96078a24ca50b9c9a38f2379e6be6d87171fcd400fc14c8934b3bce96e2f",
  },
};

export async function stageMacBinary(slices, dest) {
  if (slices.length === 1) await NodeFSP.copyFile(slices[0], dest);
  else NodeChildProcess.execFileSync("lipo", ["-create", ...slices, "-output", dest]);
}

async function downloadNode(tmp, arch, version) {
  const sum = checksums[version]?.[arch];
  if (!sum)
    throw new Error(`Pin the checksum for Node ${version} on macOS ${arch} in rune-mac.mjs.`);
  const name = `node-v${version}-darwin-${arch}`;
  const response = await fetch(`https://nodejs.org/dist/v${version}/${name}.tar.gz`, {
    signal: AbortSignal.timeout(120_000),
  });
  if (!response.ok) throw new Error(`Node ${arch} download failed (${response.status}).`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (NodeCrypto.createHash("sha256").update(bytes).digest("hex") !== sum) {
    throw new Error(`Checksum mismatch for ${name}.`);
  }
  NodeChildProcess.execFileSync(
    "tar",
    ["-xzf", "-", "-C", tmp, `${name}/bin/node`, `${name}/LICENSE`],
    { input: bytes },
  );
  return NodePath.join(tmp, name, "bin/node");
}

export async function stageMacNode({ tmp, arch, hostArch, hostNode, version, dest }) {
  const slices = [];
  for (const cpu of arch === "universal" ? ["arm64", "x64"] : [arch]) {
    let node = cpu === hostArch ? hostNode : await downloadNode(tmp, cpu, version);
    const machine = cpu === "arm64" ? "arm64" : "x86_64";
    const architectures = NodeChildProcess.execFileSync("lipo", ["-archs", node], {
      encoding: "utf8",
    })
      .trim()
      .split(/\s+/);
    if (!architectures.includes(machine))
      throw new Error(`Expected a ${machine} Node runtime at ${node}.`);
    if (architectures.length > 1) {
      const slice = NodePath.join(tmp, `node-${cpu}`);
      NodeChildProcess.execFileSync("lipo", [node, "-thin", machine, "-output", slice]);
      node = slice;
    }
    const libraries = NodeChildProcess.execFileSync("otool", ["-L", node], { encoding: "utf8" })
      .trimEnd()
      .split("\n")
      .slice(1)
      .map((line) => line.trim().split(" (")[0]);
    if (
      libraries.some((lib) => !lib.startsWith("/usr/lib/") && !lib.startsWith("/System/Library/"))
    ) {
      throw new Error(
        "Use the standalone Node distribution. This node executable links external libraries.",
      );
    }
    slices.push(node);
  }
  await stageMacBinary(slices, dest);
}
