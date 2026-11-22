import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodePath from "node:path";
import * as NodeFSP from "node:fs/promises";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import { signTrustedWindowsFile } from "../build-cli-archive.ts";
import { AzureTrustedSigningOptionsConfig } from "../desktop-packaging.ts";
import { Data, NtExecutable, NtExecutableResource, Resource } from "resedit";

export async function windowsNodeArchitecture(path, version) {
  const exe = NtExecutable.from(await NodeFSP.readFile(path), { ignoreCert: true });
  const arch = { [0x8664]: "x64", [0xaa64]: "arm64" }[exe.newHeader.fileHeader.machine];
  const info = Resource.VersionInfo.fromEntries(NtExecutableResource.from(exe).entries)[0];
  const [major, minor, patch] = version.split(".").map(Number);
  if (
    !arch ||
    !info ||
    info.fixedInfo.fileVersionMS !== major * 65536 + minor ||
    info.fixedInfo.fileVersionLS !== patch * 65536
  ) {
    throw new Error(`Use a Windows x64 or ARM64 Node ${version} distribution to package Rune.`);
  }
  return arch;
}

export async function stampWindowsExecutable(path, iconPath, version, product) {
  const exe = NtExecutable.from(await NodeFSP.readFile(path));
  const resources = NtExecutableResource.from(exe);
  const icon = Data.IconFile.from(await NodeFSP.readFile(iconPath));
  Resource.IconGroupEntry.replaceIconsForResource(
    resources.entries,
    1,
    1033,
    icon.icons.map((entry) => entry.data),
  );
  const info = Resource.VersionInfo.createEmpty();
  info.lang = 1033;
  const [numericVersion] = version.split(/[-+]/, 1);
  info.setFileVersion(numericVersion, 1033);
  info.setProductVersion(numericVersion, 1033);
  info.setStringValues(
    { lang: 1033, codepage: 1200 },
    {
      FileDescription: product,
      ProductName: product,
      CompanyName: "T3 Tools",
      ProductVersion: version,
      OriginalFilename: product + ".exe",
    },
  );
  info.outputToResourceEntries(resources.entries);
  resources.outputResource(exe);
  await NodeFSP.writeFile(path, Buffer.from(exe.generate()));
}

export function readWindowsSigning() {
  const options = Effect.runSync(AzureTrustedSigningOptionsConfig);
  if (Object.values(options).some((value) => !value.trim())) {
    throw new Error(
      "Set the Azure Trusted Signing profile, endpoint, account and publisher before signing.",
    );
  }
  return options;
}

export function signRuneWindowsFile(path, signing) {
  return Effect.runPromise(
    signTrustedWindowsFile(path, signing).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );
}

export async function signRuneWindowsPayload(app, signing) {
  const entries = await NodeFSP.opendir(app, { recursive: true });
  for await (const entry of entries) {
    if (entry.isFile() && /\.(exe|dll|node)$/i.test(entry.name)) {
      await signRuneWindowsFile(NodePath.join(entry.parentPath, entry.name), signing);
    }
  }
}

export async function buildRuneInstaller({
  app,
  artifact,
  icon,
  version,
  product,
  arch,
  tmp,
  signed,
  releaseInstall = false,
}) {
  const revision = "b2905ed52ab5be0408e2f012123110abe443aab3";
  const repository = "mozilla-firefox/firefox";
  const plugin = `https://raw.githubusercontent.com/${repository}/${revision}/other-licenses/nsis/Plugins/ExecInExplorer.dll`;
  const response = await fetch(plugin);
  if (!response.ok) throw new Error(`Could not download ExecInExplorer: HTTP ${response.status}.`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (
    NodeCrypto.hash("sha256", bytes) !==
    "0d65131958d7ef898982f5772edef1b1491ed24fcc5c454712775b7bbb8edebc"
  ) {
    throw new Error("NSIS ExecInExplorer plug-in checksum mismatch.");
  }
  await NodeFSP.writeFile(NodePath.join(tmp, "ExecInExplorer.dll"), bytes);
  await NodeFSP.writeFile(
    NodePath.join(app, "resources", "LICENSE-ExecInExplorer.txt"),
    [
      "This installer includes Mozilla's ExecInExplorer NSIS plug-in under the Mozilla Public License 2.0.",
      "https://mozilla.org/MPL/2.0/",
      `Source: https://github.com/${repository}/tree/${revision}/other-licenses/nsis/Contrib/ExecInExplorer`,
      "",
    ].join("\n"),
  );
  const files = [];
  const directories = [];
  const entries = await NodeFSP.readdir(app, { recursive: true, withFileTypes: true });
  for (const entry of entries) {
    const relative = NodePath.relative(app, NodePath.join(entry.parentPath, entry.name));
    const path = relative.split(NodePath.sep).join("\\").replaceAll("$", "$$$$");
    if (entry.isDirectory()) directories.push(path);
    else files.push(path);
  }
  directories.sort((a, b) => b.length - a.length);
  const manifest = NodePath.join(tmp, "remove-payload.nsh");
  await NodeFSP.writeFile(
    manifest,
    [
      ...files.flatMap((path) => [
        `IfFileExists "$INSTDIR\\${path}" 0 +2`,
        `Delete "$INSTDIR\\${path}"`,
      ]),
      "IfErrors busy",
      ...directories.flatMap((path) => [
        `Push "$INSTDIR\\${path}"`,
        "Call un.RemovePayloadDirectory",
      ]),
    ].join("\n"),
  );
  const defines = {
    PRODUCT: product,
    VERSION: version,
    NUMERIC_VERSION: version.split(/[-+]/, 1)[0],
    INSTALL_NAME: releaseInstall ? "T3Code.Native" : product + " Rune",
    ...(releaseInstall ? { RELEASE_INSTALL: "1", RELEASE_ARCH: arch } : {}),
    PLUGIN_DIR: tmp,
    PAYLOAD: app,
    REMOVALS: manifest,
    ICON: icon,
    ARTIFACT: artifact,
    NATIVE_ARCH: arch === "arm64" ? "${IsNativeARM64}" : "${IsNativeAMD64}",
  };
  const script = NodePath.resolve(import.meta.dirname, "../../apps/desktop/native/installer.nsi");
  NodeChildProcess.execFileSync(
    "makensis",
    [
      "-NOCD",
      ...(signed ? ["-DSIGN_UNINSTALLER"] : []),
      "-INPUTCHARSET",
      "UTF8",
      ...Object.entries(defines).map(([key, value]) => `-D${key}=${value}`),
      script,
    ],
    { cwd: NodePath.resolve(import.meta.dirname, "../.."), stdio: "inherit" },
  );
}

if (import.meta.main) {
  const file = process.argv[2];
  if (!file || process.argv.length !== 3)
    throw new Error("Expected one Windows uninstaller to sign.");
  await signRuneWindowsFile(file, readWindowsSigning());
}
