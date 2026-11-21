import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodePath from "node:path";
import {
  desktopDmgLayout,
  resolveDesktopUpdateChannel,
  stageDesktopDmgBackground,
} from "../build-desktop-artifact.ts";

const checksums = {
  arm64: "793404d0c96687e27d5ee40a668d498c92e36a64d6c2906df511031adb33cbeb",
  x64: "1664972f9cc2d6e8fce3b63e42cd30078aff602669c5856939c4519921200433",
};

export async function buildRuneDmg({ app, artifact, desktop, tmp, version }) {
  const layout = desktopDmgLayout(version);
  const channel = resolveDesktopUpdateChannel(version);
  await NodeFSP.mkdir(NodePath.join(tmp, "dmg"));
  await NodeFSP.copyFile(
    NodePath.join(desktop, "resources/dmg", `dmg-background-${channel}.svg`),
    NodePath.join(tmp, "dmg", `dmg-background-${channel}.svg`),
  );
  await Effect.runPromise(
    stageDesktopDmgBackground(tmp, channel, false).pipe(
      Effect.provide(NodeServices.layer),
      Effect.scoped,
    ),
  );
  const background = NodePath.join(tmp, "installer.tiff");
  const png = NodePath.join(tmp, layout.background);
  NodeChildProcess.execFileSync("tiffutil", [
    "-cathidpicheck",
    png,
    png.replace(/\.png$/, "@2x.png"),
    "-out",
    background,
  ]);

  const hostArch = Effect.runSync(HostProcess.Architecture);
  const cpu = hostArch === "arm64" ? "arm64" : "x86_64";
  const url = `https://github.com/electron-userland/electron-builder-binaries/releases/download/dmg-builder%401.2.5/dmgbuild-bundle-${cpu}-75c8a6c.tar.gz`;
  const response = await fetch(url, { signal: AbortSignal.timeout(90_000) });
  if (!response.ok) throw new Error(`DMG build tool download failed (${response.status}).`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (NodeCrypto.createHash("sha256").update(bytes).digest("hex") !== checksums[hostArch]) {
    throw new Error("DMG build tool checksum mismatch.");
  }
  const tools = NodePath.join(tmp, "dmg-tools");
  await NodeFSP.mkdir(tools);
  NodeChildProcess.execFileSync("tar", ["-xzf", "-", "-C", tools], { input: bytes });
  const settings = NodePath.join(tmp, "dmg.json");
  await NodeFSP.writeFile(
    settings,
    JSON.stringify({
      title: layout.title,
      background,
      icon: NodePath.join(app, "Contents/Resources/icon.icns"),
      "icon-size": layout.iconSize,
      "text-size": layout.iconTextSize,
      window: { position: { x: 400, y: 504 }, size: layout.window },
      contents: layout.contents.map((entry) => ({ ...entry, path: entry.path ?? app })),
      format: "UDZO",
      filesystem: "HFS+",
    }),
  );
  NodeChildProcess.execFileSync(
    NodePath.join(tools, "dmgbuild"),
    ["-s", settings, layout.title, artifact],
    { stdio: "inherit" },
  );
  const mount = NodePath.join(tmp, "verify-dmg");
  await NodeFSP.mkdir(mount);
  NodeChildProcess.execFileSync("hdiutil", [
    "attach",
    "-readonly",
    "-nobrowse",
    "-noautoopen",
    "-mountpoint",
    mount,
    artifact,
  ]);
  try {
    NodeChildProcess.execFileSync("codesign", [
      "--verify",
      "--strict",
      NodePath.join(mount, NodePath.basename(app)),
    ]);
  } finally {
    NodeChildProcess.execFileSync("hdiutil", ["detach", mount]);
  }
}
