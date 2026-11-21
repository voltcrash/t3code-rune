import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import * as HostProcess from "@t3tools/shared/HostProcess";
import plist from "plist";
import { build } from "vite-plus/pack";

import rootPackage from "../package.json" with { type: "json" };
import desktopPackage from "../apps/desktop/package.json" with { type: "json" };
import { applyWebBrandAssets } from "./apply-web-brand-assets.ts";
import { bundleLinuxLibatomic, stageRuntimeExternals } from "./build-cli-archive.ts";
import { stageMacIcons, stageResourceMonitor } from "./build-desktop-artifact.ts";
import { BRAND_ASSET_PATHS } from "./lib/brand-assets.ts";

const repoRoot = NodePath.resolve(import.meta.dirname, "..");
const desktop = NodePath.join(repoRoot, "apps/desktop");
const arch = Effect.runSync(HostProcess.Architecture);
const host = Effect.runSync(HostProcess.Platform);
if (!["darwin", "linux"].includes(host) || !["arm64", "x64"].includes(arch)) {
  throw new Error("Build the Rune app on a macOS or Linux x64 or ARM64 host.");
}
if (process.versions.node !== rootPackage.engines.node) {
  throw new Error(`Use Node ${rootPackage.engines.node} to build the bundled runtime.`);
}

const mac = host === "darwin";
const platform = mac ? "mac" : "linux";
const cpu = arch === "arm64" ? "aarch64" : "x86_64";
const run = (command, args, options) =>
  NodeChildProcess.execFileSync(command, args, { cwd: repoRoot, stdio: "inherit", ...options });
const node = await NodeFSP.realpath(process.execPath);
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
const rustTarget = `${cpu}-${mac ? "apple-darwin" : "unknown-linux-gnu"}`;

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
  "build",
  "--release",
  "--locked",
  "--manifest-path",
  manifest,
  "--target",
  rustTarget,
  "--target-dir",
  NodePath.join(desktop, "native/target"),
]);
run("vp", ["run", "--filter", "t3", "build"]);
const output = NodePath.join(repoRoot, "release");
await NodeFSP.mkdir(output, { recursive: true });
const tmp = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "t3-rune-"));
const published = await NodeFSP.mkdtemp(NodePath.join(output, ".rune-"));
try {
  const app = NodePath.join(tmp, mac ? "T3 Code.app" : "T3-Code.AppDir");
  const contents = NodePath.join(app, mac ? "Contents" : "usr");
  const resources = NodePath.join(contents, mac ? "Resources" : "lib/t3-code");
  const bin = NodePath.join(contents, mac ? "MacOS" : "bin");
  const helpers = mac ? NodePath.join(contents, "Helpers") : resources;
  const server = NodePath.join(resources, "server/dist");
  await NodeFSP.mkdir(bin, { recursive: true });
  await NodeFSP.mkdir(helpers, { recursive: true });
  await NodeFSP.mkdir(server, { recursive: true });
  await NodeFSP.copyFile(
    NodePath.join(desktop, "native/target", rustTarget, "release/t3-code"),
    NodePath.join(bin, "t3-code"),
  );
  await NodeFSP.copyFile(node, NodePath.join(helpers, "node"));
  await NodeFSP.copyFile(
    NodePath.resolve(node, "../../LICENSE"),
    NodePath.join(resources, "LICENSE-node"),
  );
  await NodeFSP.copyFile(NodePath.join(repoRoot, "LICENSE"), NodePath.join(resources, "LICENSE"));
  await writeRuneNotices(resources);
  await NodeFSP.cp(NodePath.join(repoRoot, "apps/server/dist"), server, {
    recursive: true,
    filter: (file) => !file.endsWith(".map"),
  });
  await NodeFSP.access(NodePath.join(server, "client/index.html"));

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
    outDir: resources,
    sourcemap: false,
    outputOptions: { codeSplitting: false },
    deps: {
      alwaysBundle: (id) =>
        !id.startsWith("node:") && !["bufferutil", "utf-8-validate"].includes(id),
      neverBundle: ["bufferutil", "utf-8-validate"],
      onlyBundle: false,
    },
  });

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
      } else {
        yield* bundleLinuxLibatomic({
          repoRoot,
          contentDir: resources,
          executablePath: NodePath.join(helpers, "node"),
        });
      }
      yield* applyWebBrandAssets("production", NodePath.join(server, "client"));
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  const file = `T3-Code-Rune-${platform}-${arch}.${mac ? "zip" : "AppImage"}`;
  if (mac) {
    await NodeFSP.writeFile(
      NodePath.join(contents, "Info.plist"),
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
