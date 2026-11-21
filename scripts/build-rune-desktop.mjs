import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeUtil from "node:util";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import * as HostProcess from "@t3tools/shared/HostProcess";
import { resolveSpawnCommand } from "@t3tools/shared/shell";
import plist from "plist";
import { bundleRuneWorker } from "../apps/desktop/scripts/bundle-rune-worker.mjs";

import rootPackage from "../package.json" with { type: "json" };
import desktopPackage from "../apps/desktop/package.json" with { type: "json" };
import { applyWebBrandAssets } from "./apply-web-brand-assets.ts";
import { bundleLinuxLibatomic, stageRuntimeExternals } from "./build-cli-archive.ts";
import {
  resolveResourceMonitorRustTargets,
  stageMacIcons,
  stageResourceMonitor,
} from "./build-desktop-artifact.ts";
import { BRAND_ASSET_PATHS } from "./lib/brand-assets.ts";
import { windowsNodeArchitecture, stampWindowsExecutable } from "./lib/rune-windows.mjs";
import { windowsSystemTar } from "./lib/windows-tar.ts";

const repoRoot = NodePath.resolve(import.meta.dirname, "..");
const desktop = NodePath.join(repoRoot, "apps/desktop");
const host = Effect.runSync(HostProcess.Platform);
const { values } = NodeUtil.parseArgs({ options: { "windows-node-dir": { type: "string" } } });
const windowsNodeDir = values["windows-node-dir"];
if (windowsNodeDir !== undefined && (!windowsNodeDir.trim() || host !== "linux")) {
  throw new Error(
    "--windows-node-dir takes an extracted Windows Node distribution on a Linux host.",
  );
}
const platform = windowsNodeDir ? "win" : { darwin: "mac", linux: "linux", win32: "win" }[host];
const node = await NodeFSP.realpath(
  windowsNodeDir ? NodePath.join(windowsNodeDir, "node.exe") : process.execPath,
);
const arch =
  platform === "win"
    ? await windowsNodeArchitecture(node, rootPackage.engines.node)
    : Effect.runSync(HostProcess.Architecture);
if (!platform || !["arm64", "x64"].includes(arch)) {
  throw new Error("Build the Rune app on a macOS, Linux or Windows x64 or ARM64 host.");
}
if (process.versions.node !== rootPackage.engines.node) {
  throw new Error(`Use Node ${rootPackage.engines.node} to build the bundled runtime.`);
}

const mac = platform === "mac";
const win = platform === "win";
const cpu = arch === "arm64" ? "aarch64" : "x86_64";
const run = (command, args, options) =>
  NodeChildProcess.execFileSync(command, args, { cwd: repoRoot, stdio: "inherit", ...options });
if (mac) {
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
}

const manifest = NodePath.join(desktop, "native/Cargo.toml");
const rustTarget = windowsNodeDir
  ? { x64: "x86_64-pc-windows-gnu", arm64: "aarch64-pc-windows-gnullvm" }[arch]
  : resolveResourceMonitorRustTargets(platform, arch)[0];

async function writeRuneNotices(resources) {
  const metadata = JSON.parse(
    NodeChildProcess.execFileSync(
      "cargo",
      [
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        manifest,
        "--filter-platform",
        rustTarget,
      ],
      { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
    ),
  );
  const rune = NodePath.dirname(metadata.packages.find((pkg) => pkg.name === "rune").manifest_path);
  const notices = [
    ["Rune", NodePath.resolve(rune, "../../LICENSE")],
    ["Rune syntax highlighting", NodePath.resolve(rune, "../rune-highlights/vendor/LICENSE.md")],
    ["Inter", NodePath.join(rune, "assets/Inter-LICENSE.txt")],
    ["IBM Plex Mono", NodePath.join(rune, "assets/IBMPlexMono-LICENSE.txt")],
  ];
  const text = await Promise.all(
    notices.map(async ([title, file]) => title + "\n\n" + (await NodeFSP.readFile(file, "utf8"))),
  );
  await NodeFSP.writeFile(NodePath.join(resources, "LICENSE-Rune.txt"), text.join("\n\n"));
}

async function writeAppImageRuntime(tmp) {
  const checksums = {
    aarch64: "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
    x86_64: "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
  };
  const url = `https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-${cpu}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(60_000) });
  if (!response.ok) throw new Error(`AppImage runtime download failed (${response.status}).`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (NodeCrypto.createHash("sha256").update(bytes).digest("hex") !== checksums[cpu]) {
    throw new Error(`AppImage runtime checksum mismatch for ${cpu}.`);
  }
  const path = NodePath.join(tmp, "appimage-runtime");
  await NodeFSP.writeFile(path, bytes);
  return path;
}

run("cargo", [
  win ? "rustc" : "build",
  "--bin",
  "t3-code",
  "--release",
  "--locked",
  "--manifest-path",
  manifest,
  "--target",
  rustTarget,
  "--target-dir",
  NodePath.join(desktop, "native/target"),
  // PE resource editing requires a shell executable without a COFF symbol table.
  ...(win ? ["--", "-Cstrip=symbols"] : []),
]);
const vp = await Effect.runPromise(
  resolveSpawnCommand("vp", ["run", "--filter", "t3", "build"]).pipe(
    Effect.provide(NodeServices.layer),
  ),
);
run(vp.command, vp.args, { shell: vp.shell });
const output = NodePath.join(repoRoot, "release");
await NodeFSP.mkdir(output, { recursive: true });
const tmp = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "t3-rune-"));
const published = await NodeFSP.mkdtemp(NodePath.join(output, ".rune-"));
try {
  const layout = {
    mac: {
      root: "T3 Code.app",
      bin: "Contents/MacOS",
      resources: "Contents/Resources",
      helpers: "Contents/Helpers",
    },
    linux: {
      root: "T3-Code.AppDir",
      bin: "usr/bin",
      resources: "usr/lib/t3-code",
      helpers: "usr/lib/t3-code",
    },
    win: { root: "T3 Code", bin: ".", resources: "resources", helpers: "resources" },
  }[platform];
  const app = NodePath.join(tmp, layout.root);
  const resources = NodePath.join(app, layout.resources);
  const bin = NodePath.join(app, layout.bin);
  const helpers = NodePath.join(app, layout.helpers);
  const executable = NodePath.join(bin, win ? "T3 Code.exe" : "t3-code");
  const server = NodePath.join(resources, "server/dist");
  await NodeFSP.mkdir(bin, { recursive: true });
  await NodeFSP.mkdir(helpers, { recursive: true });
  await NodeFSP.mkdir(server, { recursive: true });
  await NodeFSP.copyFile(
    NodePath.join(desktop, "native/target", rustTarget, "release", win ? "t3-code.exe" : "t3-code"),
    executable,
  );
  await NodeFSP.copyFile(node, NodePath.join(helpers, win ? "node.exe" : "node"));
  await NodeFSP.copyFile(
    NodePath.resolve(NodePath.dirname(node), win ? "LICENSE" : "../LICENSE"),
    NodePath.join(resources, "LICENSE-node"),
  );
  await NodeFSP.copyFile(NodePath.join(repoRoot, "LICENSE"), NodePath.join(resources, "LICENSE"));
  await writeRuneNotices(resources);
  await NodeFSP.cp(NodePath.join(repoRoot, "apps/server/dist"), server, {
    recursive: true,
    filter: (file) => !file.endsWith(".map"),
  });
  await NodeFSP.access(NodePath.join(server, "client/index.html"));

  await bundleRuneWorker(resources);

  await Effect.runPromise(
    Effect.gen(function* () {
      yield* stageRuntimeExternals({
        repoRoot,
        stageDir: resources,
        platform,
        arch,
        version: desktopPackage.version,
      });
      yield* stageResourceMonitor({
        repoRoot,
        stageResourcesDir: server,
        rustTarget,
        platform,
        arch,
        verbose: false,
      });
      if (mac) {
        yield* stageMacIcons(
          resources,
          NodePath.join(repoRoot, BRAND_ASSET_PATHS.productionMacIconPng),
          false,
        );
      } else if (platform === "linux") {
        yield* bundleLinuxLibatomic({
          repoRoot,
          contentDir: resources,
          executablePath: NodePath.join(helpers, "node"),
        });
      }
      yield* applyWebBrandAssets("production", NodePath.join(server, "client"));
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  const file = `T3-Code-Rune-${platform}-${arch}.${platform === "linux" ? "AppImage" : "zip"}`;
  if (mac) {
    await NodeFSP.writeFile(
      NodePath.join(app, "Contents/Info.plist"),
      plist.build({
        CFBundlePackageType: "APPL",
        CFBundleIdentifier: "com.t3tools.t3code.rune",
        CFBundleName: "T3 Code",
        CFBundleExecutable: "t3-code",
        CFBundleIconFile: "icon.icns",
        CFBundleShortVersionString: desktopPackage.version,
        CFBundleVersion: desktopPackage.version,
        LSApplicationCategoryType: "public.app-category.developer-tools",
        NSHighResolutionCapable: true,
      }),
    );
    run("codesign", ["--force", "--sign", "-", app]);
    run("codesign", ["--verify", "--strict", app]);
    run("ditto", ["-c", "-k", "--keepParent", app, NodePath.join(published, file)]);
  } else if (win) {
    await stampWindowsExecutable(
      executable,
      NodePath.join(repoRoot, BRAND_ASSET_PATHS.productionWindowsIconIco),
      desktopPackage.version,
    );
    const archive = NodePath.join(published, file);
    if (host === "win32") {
      run(windowsSystemTar(), ["-a", "-cf", archive, "-C", tmp, layout.root]);
    } else {
      run("zip", ["-q", "-r", "-X", archive, layout.root], { cwd: tmp });
    }
  } else {
    const desktopId = "com.t3tools.T3Code.Rune";
    const icon = `usr/share/icons/hicolor/1024x1024/apps/${desktopId}.png`;
    const entry = `usr/share/applications/${desktopId}.desktop`;
    await NodeFSP.mkdir(NodePath.dirname(NodePath.join(app, icon)), { recursive: true });
    await NodeFSP.mkdir(NodePath.dirname(NodePath.join(app, entry)), { recursive: true });
    await NodeFSP.copyFile(
      NodePath.join(repoRoot, BRAND_ASSET_PATHS.productionLinuxIconPng),
      NodePath.join(app, icon),
    );
    await NodeFSP.copyFile(
      NodePath.join(desktop, "native/t3-code.desktop"),
      NodePath.join(app, entry),
    );
    for (const [name, target] of [
      ["AppRun", "usr/bin/t3-code"],
      [`${desktopId}.desktop`, entry],
      [`${desktopId}.png`, icon],
      [".DirIcon", `${desktopId}.png`],
    ]) {
      await NodeFSP.symlink(target, NodePath.join(app, name));
    }
    run(
      "appimagetool",
      [
        "--no-appstream",
        "--runtime-file",
        await writeAppImageRuntime(tmp),
        app,
        NodePath.join(published, file),
      ],
      {
        env: { ...process.env, ARCH: cpu },
      },
    );
  }
  await NodeFSP.rename(NodePath.join(published, file), NodePath.join(output, file));
  console.log(NodePath.join(output, file));
} finally {
  await NodeFSP.rm(tmp, { recursive: true, force: true });
  await NodeFSP.rm(published, { recursive: true, force: true });
}
