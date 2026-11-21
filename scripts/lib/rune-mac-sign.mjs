import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import { notarizeMacArtifact } from "../build-cli-archive.ts";

const run = (cmd, args) => NodeChildProcess.execFileSync(cmd, args, { stdio: "inherit" });

export function readMacSigning() {
  const selected = process.env.CSC_NAME?.trim() || process.env.T3CODE_CLI_MAC_SIGN_IDENTITY?.trim();
  const identities = NodeChildProcess.execFileSync(
    "security",
    ["find-identity", "-v", "-p", "codesigning"],
    { encoding: "utf8" },
  );
  const matches = [
    ...identities.matchAll(/^\s*\d+\) ([0-9a-f]{40}) "(Developer ID Application: [^"]+)"/gim),
  ].filter(
    ([, hash, name]) =>
      !selected || hash.toLowerCase() === selected.toLowerCase() || name === selected,
  );
  if (matches.length !== 1) {
    throw new Error(
      "Select an installed Developer ID Application certificate with CSC_NAME (full name or SHA-1 fingerprint).",
    );
  }
  const credentials = [
    process.env.APPLE_API_KEY,
    process.env.APPLE_API_KEY_ID,
    process.env.APPLE_API_ISSUER,
  ].map((value) => value?.trim());
  if (credentials.some(Boolean) && !credentials.every(Boolean)) {
    throw new Error(
      "Set APPLE_API_KEY, APPLE_API_KEY_ID and APPLE_API_ISSUER together for notarization.",
    );
  }
  return {
    identity: matches[0][1],
    notary: credentials[0]
      ? ["--key", credentials[0], "--key-id", credentials[1], "--issuer", credentials[2]]
      : undefined,
  };
}

const machO = new Set([
  "feedface",
  "cefaedfe",
  "feedfacf",
  "cffaedfe",
  "cafebabe",
  "bebafeca",
  "cafebabf",
  "bfbafeca",
]);

async function nativeDependencies(directory) {
  const files = [];
  const entries = await NodeFSP.opendir(directory, { recursive: true });
  for await (const entry of entries) {
    if (!entry.isFile()) continue;
    const path = NodePath.join(entry.parentPath, entry.name);
    await using file = await NodeFSP.open(path);
    const magic = Buffer.alloc(4);
    const { bytesRead } = await file.read(magic, 0, magic.length, 0);
    if (bytesRead === magic.length && machO.has(magic.toString("hex"))) files.push(path);
  }
  return files;
}

function sign(file, identity, extra = []) {
  run("codesign", ["--force", "--sign", identity, "--timestamp", ...extra, file]);
}

async function notarize(file, credentials) {
  await Effect.runPromise(
    notarizeMacArtifact(file, credentials).pipe(Effect.provide(NodeServices.layer)),
  );
}

function staple(file) {
  run("xcrun", ["stapler", "staple", file]);
  run("xcrun", ["stapler", "validate", file]);
}

export async function signRuneMacApp({ app, resources, node, repoRoot, signing }) {
  if (signing) {
    // Preview JS is untrusted. Only the packaging job supplies the native dependencies and monitor.
    const binaries = await nativeDependencies(NodePath.join(resources, "node_modules"));
    binaries.push(NodePath.join(resources, "server/dist/resource-monitor/t3-resource-monitor"));
    for (const binary of binaries) sign(binary, signing.identity, ["--options", "runtime"]);
    sign(node, signing.identity, [
      "--options",
      "runtime",
      "--entitlements",
      NodePath.join(repoRoot, "apps/server/resources/cli-entitlements.plist"),
    ]);
    sign(app, signing.identity, ["--options", "runtime"]);
    run("codesign", ["--verify", "--strict", "--all-architectures", ...binaries, node]);
  } else {
    run("codesign", ["--force", "--sign", "-", app]);
  }
  run("codesign", ["--verify", "--deep", "--strict", "--all-architectures", app]);
  if (signing && !signing.notary) {
    console.log(
      "Signed with Developer ID. Set APPLE_API_KEY, APPLE_API_KEY_ID and APPLE_API_ISSUER to request notarization.",
    );
  }
}

export async function stapleRuneMacApp({ app, tmp, signing, submittedInDmg }) {
  if (signing?.notary) {
    // An accepted DMG submission includes a ticket for its signed app.
    if (!submittedInDmg) {
      const zip = NodePath.join(tmp, "notarize-app.zip");
      run("ditto", ["-c", "-k", "--keepParent", app, zip]);
      await notarize(zip, signing.notary);
    }
    staple(app);
  }
}

export async function signRuneDmg(artifact, signing) {
  sign(artifact, signing.identity);
  run("codesign", ["--verify", "--strict", artifact]);
  if (signing.notary) {
    await notarize(artifact, signing.notary);
    staple(artifact);
  }
}
