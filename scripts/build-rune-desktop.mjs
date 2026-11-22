import * as NodeChildProcess from "node:child_process";
import * as NodeCrypto from "node:crypto";
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeUtil from "node:util";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Config from "effect/Config";
import * as Effect from "effect/Effect";
import * as HostProcess from "@t3tools/shared/HostProcess";
import { resolveSpawnCommand } from "@t3tools/shared/shell";
import plist from "plist";
import {
  bundleRuneWorker,
  readPrebuiltRuneWorker,
} from "../apps/desktop/scripts/bundle-rune-worker.mjs";

import rootPackage from "../package.json" with { type: "json" };
import { applyWebBrandAssets } from "./apply-web-brand-assets.ts";
import { bundleLinuxLibatomic, stageRuntimeExternals } from "./build-cli-archive.ts";
import {
  DESKTOP_APP_ID,
  resolveDesktopBuildIconAssets,
  resolveDesktopProductName,
  resolveGitHubPublishConfig,
  resolveDesktopUpdateChannel,
  resolveDesktopWebAssetBrand,
  resolveResourceMonitorRustTargets,
  withDesktopBuildLock,
  stageMacIcons,
  stageResourceMonitor,
  stageWslRuntimeArchive,
  validatePackagedWslRuntime,
  WSL_RUNTIME_ARCHIVE_NAME,
  WSL_RUNTIME_ARCHIVE_HASH_NAME,
} from "./desktop-packaging.ts";
import { getDefaultBuildArch } from "./lib/build-target-arch.ts";
import { resolveTaggedSourceVersion } from "./lib/tagged-source-version.ts";
import { stageMacBinary, stageMacNode, stagePrebuiltMacLauncher } from "./lib/rune-mac.mjs";
import { publishRuneArtifacts, publishUnpackedRuneApp } from "./lib/rune-artifacts.mjs";
import { buildRuneDeb } from "./lib/rune-deb.mjs";
import {
  stapleRuneMacApp,
  readMacSigning,
  signRuneMacApp,
  signRuneDmg,
} from "./lib/rune-mac-sign.mjs";
import { buildRuneDmg } from "./lib/rune-dmg.mjs";
import {
  buildRuneInstaller,
  readWindowsSigning,
  signRuneWindowsFile,
  signRuneWindowsPayload,
  windowsNodeArchitecture,
  stampWindowsExecutable,
} from "./lib/rune-windows.mjs";
import { windowsSystemTar } from "./lib/windows-tar.ts";

const repoRoot = NodePath.resolve(import.meta.dirname, "..");
const desktop = NodePath.join(repoRoot, "apps/desktop");
const host = Effect.runSync(HostProcess.Platform);
const { values } = NodeUtil.parseArgs({
  allowNegative: true,
  options: {
    help: { type: "boolean", short: "h" },
    distribution: { type: "boolean" },
    "mock-updates": { type: "boolean" },
    "mock-update-server-port": { type: "string" },
    arch: { type: "string" },
    "build-version": { type: "string" },
    "keep-stage": { type: "boolean" },
    verbose: { type: "boolean" },
    "output-dir": { type: "string" },
    "update-feed": { type: "boolean" },
    platform: { type: "string" },
    "skip-build": { type: "boolean" },
    "prebuilt-mac-launcher": { type: "boolean" },
    signed: { type: "boolean" },
    target: { type: "string" },
    "windows-node-dir": { type: "string" },
    "wsl-runtime": { type: "string" },
  },
});
if (values.help) {
  console.log(`Build the native T3 Code desktop distribution.

Usage: node scripts/build-rune-desktop.mjs [options]

  --distribution                Build the standard installer set (dist:desktop:artifact)
                                macOS dmg + ZIP, Linux AppImage + deb, Windows NSIS
  --platform mac|linux|win      Target platform (defaults to the host)
  --arch x64|arm64|universal    Architecture (universal is macOS only)
  --target <format>             macOS: dmg, zip, dmg,zip, dir
                                Linux: AppImage, deb, AppImage,deb, dir
                                Windows: nsis, zip, nsis,zip, dir
  --windows-node-dir <dir>      Extracted target Node distribution for Windows or Linux hosts
  --wsl-runtime <archive>       Bundle the matching Linux CLI release archive for WSL on Windows
  --build-version <version>     Version to embed in the app and artifact names
  --output-dir <dir>            Directory for completed artifacts (default: release)
  --signed                      Sign the macOS or Windows distribution
  --update-feed                 Stage update metadata for macOS ZIPs, Linux AppImage,deb, or Windows NSIS
  --skip-build                  Use the matching prebuilt server, client and desktop worker
  --prebuilt-mac-launcher       Also use the macOS preview launcher in the prebuilt bundle
  --keep-stage                  Keep staged packaging files
  --verbose                     Show detailed build output
  -h, --help                    Show this help`);
  process.exit(0);
}
function booleanOption(name, env) {
  return values[name] ?? Effect.runSync(Config.Boolean(env).pipe(Config.withDefault(false)));
}
if (
  values["mock-updates"] ||
  (values["mock-update-server-port"] !== undefined && values["mock-updates"] !== false) ||
  (values.distribution && booleanOption("mock-updates", "T3CODE_DESKTOP_MOCK_UPDATES"))
) {
  throw new Error(
    "Use node scripts/build-desktop-artifact.ts --mock-updates to exercise the Electron updater.",
  );
}
const skipBuild = booleanOption("skip-build", "T3CODE_DESKTOP_SKIP_BUILD");
const keepStage = booleanOption("keep-stage", "T3CODE_DESKTOP_KEEP_STAGE");
const verbose = booleanOption("verbose", "T3CODE_DESKTOP_VERBOSE");
const signed = booleanOption("signed", "T3CODE_DESKTOP_SIGNED");
const windowsNodeDir = values["windows-node-dir"];
if (
  windowsNodeDir !== undefined &&
  (!windowsNodeDir.trim() || !["linux", "win32"].includes(host))
) {
  throw new Error(
    "--windows-node-dir takes an extracted Windows Node distribution on a Windows or Linux host.",
  );
}
const crossWindows = host === "linux" && windowsNodeDir !== undefined;
const platform = windowsNodeDir ? "win" : { darwin: "mac", linux: "linux", win32: "win" }[host];
const requestedPlatform = values.platform ?? process.env.T3CODE_DESKTOP_PLATFORM;
if (requestedPlatform !== undefined && requestedPlatform !== platform) {
  throw new Error(`Build the ${requestedPlatform} Rune package on a matching host.`);
}
const wslRuntime =
  values["wsl-runtime"] ??
  (platform === "win" ? process.env.T3CODE_DESKTOP_WSL_RUNTIME : undefined);
if (wslRuntime !== undefined && (platform !== "win" || !wslRuntime.trim())) {
  throw new Error("--wsl-runtime requires a Linux CLI archive path and a Windows target.");
}
const prebuiltMacLauncher = values["prebuilt-mac-launcher"] ?? false;
if (prebuiltMacLauncher && (platform !== "mac" || !skipBuild)) {
  throw new Error("--prebuilt-mac-launcher requires a macOS package with --skip-build.");
}
const distributionTargets =
  { mac: ["dmg", "zip"], linux: ["AppImage", "deb"], win: ["nsis"] }[platform] ?? [];
const portableTarget = platform === "linux" ? "AppImage" : "zip";
const selectedTarget =
  values.target ??
  process.env.T3CODE_DESKTOP_TARGET ??
  (values.distribution ? distributionTargets[0] : portableTarget);
const target =
  values.distribution && selectedTarget === distributionTargets[0]
    ? distributionTargets.join(",")
    : selectedTarget;
if (
  !{
    mac: ["dmg", "zip", "dmg,zip", "dir"],
    linux: ["AppImage", "deb", "AppImage,deb", "dir"],
    win: ["zip", "nsis", "nsis,zip", "dir"],
  }[platform]?.includes(target)
) {
  throw new Error(`Unsupported Rune package target "${target}" for ${platform ?? host}.`);
}
const node = await NodeFSP.realpath(
  windowsNodeDir ? NodePath.join(windowsNodeDir, "node.exe") : process.execPath,
);
const hostArch = Effect.runSync(HostProcess.Architecture);
const runtimeArch =
  platform === "win" ? await windowsNodeArchitecture(node, rootPackage.engines.node) : hostArch;
const arch =
  values.arch ??
  process.env.T3CODE_DESKTOP_ARCH ??
  (values.distribution && host === "win32" && !windowsNodeDir
    ? Effect.runSync(getDefaultBuildArch("win", { archChoices: ["x64", "arm64"] }))
    : runtimeArch);
if (!platform || !["arm64", "x64"].includes(runtimeArch)) {
  throw new Error("Build the Rune app on a macOS, Linux or Windows x64 or ARM64 host.");
}
if (platform === "mac" ? !["arm64", "x64", "universal"].includes(arch) : arch !== runtimeArch) {
  if (platform === "win" && ["x64", "arm64"].includes(arch)) {
    throw new Error(
      `The Windows ${arch} package requires ${arch} Node. Use --windows-node-dir with an extracted ${arch} distribution, or select --arch ${runtimeArch} for this runtime.`,
    );
  }
  throw new Error(
    `Unsupported Rune architecture "${arch}" for the ${platform} ${runtimeArch} runtime.`,
  );
}
if (process.versions.node !== rootPackage.engines.node) {
  throw new Error(`Use Node ${rootPackage.engines.node} to build the bundled runtime.`);
}

const output = NodePath.resolve(
  repoRoot,
  values["output-dir"] ?? process.env.T3CODE_DESKTOP_OUTPUT_DIR ?? "release",
);
const mac = platform === "mac";
const win = platform === "win";
const formats = target.split(",");
const updateFeed = values["update-feed"] ?? false;
const linuxUpdate = updateFeed && platform === "linux";
if (
  updateFeed &&
  !(mac && formats.includes("zip")) &&
  !(win && target === "nsis") &&
  !(linuxUpdate && target === "AppImage,deb")
) {
  throw new Error(
    "--update-feed requires a macOS ZIP, Linux AppImage,deb, or Windows nsis target.",
  );
}
const buildDeb = formats.includes("deb");
const cpu = arch === "arm64" ? "aarch64" : "x86_64";
const run = (command, args, options) =>
  NodeChildProcess.execFileSync(command, args, { cwd: repoRoot, stdio: "inherit", ...options });
const manifest = NodePath.join(desktop, "native/Cargo.toml");
const rustTargets = crossWindows
  ? [{ x64: "x86_64-pc-windows-gnu", arm64: "aarch64-pc-windows-gnullvm" }[arch]]
  : resolveResourceMonitorRustTargets(platform, arch);

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
        rustTargets[0],
      ],
      { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
    ),
  );
  const rune = NodePath.dirname(metadata.packages.find((pkg) => pkg.name === "rune").manifest_path);
  const notices = [
    ["Rune", NodePath.resolve(rune, "../../LICENSE")],
    ["Rune syntax highlighting", NodePath.resolve(rune, "../rune-highlights/vendor/LICENSE.md")],
    ["Lucide", NodePath.join(desktop, "node_modules/lucide/LICENSE")],
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

async function buildRunePackage() {
  const override = values["build-version"] ?? process.env.T3CODE_DESKTOP_VERSION;
  const version =
    override === undefined
      ? resolveTaggedSourceVersion(
          repoRoot,
          JSON.parse(await NodeFSP.readFile(NodePath.join(desktop, "package.json"), "utf8"))
            .version,
        )
      : override.trim();
  if (
    !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.test(
      version,
    )
  ) {
    throw new Error("Specify a release version such as 1.2.3 or 1.2.3-nightly.20261121.42.");
  }
  if (signed && !mac && !win) throw new Error("Rune signing supports macOS and Windows builds.");
  const channel = resolveDesktopUpdateChannel(version);
  if (
    updateFeed &&
    channel !== "nightly" &&
    /^\d+\.\d+\.\d+-(?:nightly|preview)(?:[.+]|$)/.test(version)
  ) {
    throw new Error("Preview builds and malformed nightly versions cannot publish update feeds.");
  }
  const signing = signed && mac ? readMacSigning() : undefined;
  const windowsSigning = signed && win ? readWindowsSigning() : undefined;
  const product = resolveDesktopProductName(version);
  const icons = resolveDesktopBuildIconAssets(version);
  const brand = resolveDesktopWebAssetBrand(version);
  const serverDist = NodePath.join(repoRoot, "apps/server/dist");
  if (!skipBuild) {
    const vp = await Effect.runPromise(
      resolveSpawnCommand("vp", ["run", "--filter", "t3", "build"]).pipe(
        Effect.provide(NodeServices.layer),
      ),
    );
    run(vp.command, vp.args, { shell: vp.shell, env: { ...process.env, APP_VERSION: version } });
  }
  await NodeFSP.mkdir(output, { recursive: true });
  await using cleanup = new AsyncDisposableStack();
  const staging = await NodeFSP.mkdtempDisposable(NodePath.join(NodeOS.tmpdir(), "t3-rune-"));
  if (keepStage) console.log(`Keeping staged Rune package in ${staging.path}`);
  else cleanup.use(staging);
  await using publication = await NodeFSP.mkdtempDisposable(NodePath.join(output, ".rune-"));
  const tmp = staging.path;
  const published = publication.path;
  const layout = {
    mac: {
      root: product + ".app",
      bin: "Contents/MacOS",
      resources: "Contents/Resources",
      helpers: "Contents/Helpers",
    },
    linux: {
      root: buildDeb ? `debian/${linuxUpdate ? "t3code" : "t3code-rune"}` : "T3-Code.AppDir",
      bin: linuxUpdate ? `opt/${product}` : "usr/bin",
      resources: linuxUpdate ? `opt/${product}/resources` : "usr/lib/t3-code",
      helpers: linuxUpdate ? `opt/${product}/resources` : "usr/lib/t3-code",
    },
    win: { root: product, bin: ".", resources: "resources", helpers: "resources" },
  }[platform];
  const unpacked = target === "dir";
  const unpackedName = platform + (arch === "x64" ? "" : `-${arch}`) + (mac ? "" : "-unpacked");
  const unpackedDir = NodePath.join(published, unpackedName);
  const unpackedApp = mac ? NodePath.join(unpackedDir, layout.root) : unpackedDir;
  const app = unpacked && !keepStage ? unpackedApp : NodePath.join(tmp, layout.root);
  const resources = NodePath.join(app, layout.resources);
  const bin = NodePath.join(app, layout.bin);
  const helpers = NodePath.join(app, layout.helpers);
  const binaryName = linuxUpdate ? "t3code" : "t3-code";
  const executable = NodePath.join(bin, win ? product + ".exe" : binaryName);
  const server = NodePath.join(resources, "server/dist");
  await NodeFSP.mkdir(bin, { recursive: true });
  await NodeFSP.mkdir(helpers, { recursive: true });
  await NodeFSP.mkdir(server, { recursive: true });
  const publishConfig = updateFeed
    ? await Effect.runPromise(resolveGitHubPublishConfig(channel))
    : undefined;
  const writeReleaseSource = (format) =>
    NodeFSP.writeFile(
      NodePath.join(resources, "rune-release.json"),
      JSON.stringify({
        repository: publishConfig ? `${publishConfig.owner}/${publishConfig.repo}` : null,
        arch,
        format,
      }),
    );
  if (updateFeed) await writeReleaseSource(mac ? "zip" : win ? "exe" : "deb");
  if (wslRuntime !== undefined) {
    await Effect.runPromise(
      Effect.gen(function* () {
        yield* stageWslRuntimeArchive({
          sourceArchivePath: NodePath.resolve(wslRuntime),
          archivePath: NodePath.join(resources, WSL_RUNTIME_ARCHIVE_NAME),
          hashPath: NodePath.join(resources, WSL_RUNTIME_ARCHIVE_HASH_NAME),
        });
        yield* validatePackagedWslRuntime({
          packagedAppDir: app,
          appVersion: version,
          targetArch: arch,
        });
      }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
    );
  }
  await NodeFSP.cp(serverDist, server, {
    recursive: true,
    filter: (file) => !file.endsWith(".map"),
  });
  await NodeFSP.access(NodePath.join(server, "client/index.html"));

  if (skipBuild)
    await NodeFSP.writeFile(
      NodePath.join(resources, "rune-projects.mjs"),
      await readPrebuiltRuneWorker(version, server),
    );
  else await bundleRuneWorker(resources, version);

  const bundledNode = NodePath.join(helpers, win ? "node.exe" : "node");
  if (prebuiltMacLauncher) {
    const source = NodePath.join(desktop, "dist/rune-bundle/t3-code");
    await stagePrebuiltMacLauncher(source, executable, arch);
  } else {
    for (const rustTarget of rustTargets)
      run(
        "cargo",
        [
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
        ],
        { env: { ...process.env, T3CODE_RUNE_PRODUCT_NAME: product } },
      );
    const binaries = rustTargets.map((rustTarget) =>
      NodePath.join(
        desktop,
        "native/target",
        rustTarget,
        "release",
        win ? "t3-code.exe" : "t3-code",
      ),
    );
    if (mac) await stageMacBinary(binaries, executable);
    else await NodeFSP.copyFile(binaries[0], executable);
  }
  if (mac) {
    await stageMacNode({
      tmp,
      arch,
      hostArch,
      hostNode: node,
      version: rootPackage.engines.node,
      dest: bundledNode,
    });
  } else {
    await NodeFSP.copyFile(node, bundledNode);
  }
  await NodeFSP.copyFile(
    NodePath.resolve(NodePath.dirname(node), win ? "LICENSE" : "../LICENSE"),
    NodePath.join(resources, "LICENSE-node"),
  );
  await NodeFSP.copyFile(NodePath.join(repoRoot, "LICENSE"), NodePath.join(resources, "LICENSE"));
  await writeRuneNotices(resources);
  await Effect.runPromise(
    Effect.gen(function* () {
      yield* stageRuntimeExternals({
        repoRoot,
        stageDir: resources,
        platform,
        arch,
        version,
      });
      yield* stageResourceMonitor({
        repoRoot,
        stageResourcesDir: server,
        ...(crossWindows ? { rustTarget: rustTargets[0] } : {}),
        platform,
        arch,
        verbose,
      });
      if (mac) {
        yield* stageMacIcons(resources, NodePath.join(repoRoot, icons.macIconPng), false);
      }
      yield* applyWebBrandAssets(brand, NodePath.join(server, "client"));
    }).pipe(Effect.scoped, Effect.provide(NodeServices.layer)),
  );

  const artifactName = (format) => {
    if (linuxUpdate && format === "AppImage") {
      const suffix = channel === "nightly" ? "-Nightly" : "";
      return `T3-Code${suffix}-${arch === "x64" ? "x86_64" : arch}.AppImage`;
    }
    const artifactArch = format === "deb" && arch === "x64" ? "amd64" : arch;
    const extension = format === "nsis" ? "exe" : format;
    if (updateFeed) return `T3-Code-${version}-${artifactArch}.${extension}`;
    return `T3-Code-Rune-${version}-${platform}-${artifactArch}.${extension}`;
  };
  if (mac) {
    await NodeFSP.writeFile(
      NodePath.join(app, "Contents/Info.plist"),
      plist.build({
        CFBundlePackageType: "APPL",
        CFBundleIdentifier: updateFeed ? DESKTOP_APP_ID : "com.t3tools.t3code.rune",
        ...(updateFeed && {
          CFBundleURLTypes: [
            { CFBundleURLName: "T3 Code", CFBundleURLSchemes: ["t3code", "t3code-dev"] },
          ],
        }),
        CFBundleName: product,
        CFBundleDisplayName: product,
        CFBundleExecutable: "t3-code",
        CFBundleIconFile: "icon.icns",
        CFBundleShortVersionString: version,
        CFBundleVersion: version,
        LSApplicationCategoryType: "public.app-category.developer-tools",
        NSHighResolutionCapable: true,
      }),
    );
    await NodeFSP.symlink(binaryName, NodePath.join(bin, product));
    await signRuneMacApp({ app, resources, node: bundledNode, repoRoot, signing });
    const buildDmg = formats.includes("dmg");
    if (buildDmg) {
      const artifact = NodePath.join(published, artifactName("dmg"));
      await buildRuneDmg({ app, artifact, desktop, tmp, version });
      if (signing) await signRuneDmg(artifact, signing);
    }
    if (unpacked || formats.includes("zip")) {
      await stapleRuneMacApp({ app, tmp, signing, submittedInDmg: buildDmg });
      if (!unpacked) {
        run("ditto", [
          "-c",
          "-k",
          "--keepParent",
          app,
          NodePath.join(published, artifactName("zip")),
        ]);
      }
    }
  } else if (win) {
    await stampWindowsExecutable(
      executable,
      NodePath.join(repoRoot, icons.windowsIconIco),
      version,
      product,
    );
    if (windowsSigning) await signRuneWindowsPayload(app, windowsSigning);
    if (formats.includes("nsis")) {
      const archive = NodePath.join(published, artifactName("nsis"));
      await buildRuneInstaller({
        app,
        artifact: archive,
        icon: NodePath.join(repoRoot, icons.windowsIconIco),
        version,
        product,
        arch,
        tmp,
        signed,
        releaseInstall: updateFeed,
      });
      if (windowsSigning) await signRuneWindowsFile(archive, windowsSigning);
    }
    if (formats.includes("zip")) {
      const archive = NodePath.join(published, artifactName("zip"));
      if (host === "win32") {
        run(windowsSystemTar(), ["-a", "-cf", archive, "-C", tmp, layout.root]);
      } else {
        run("zip", ["-q", "-r", "-X", archive, layout.root], { cwd: tmp });
      }
    }
  } else {
    const desktopId = linuxUpdate ? "t3code" : "com.t3tools.T3Code.Rune";
    const icon = `usr/share/icons/hicolor/512x512/apps/${desktopId}.png`;
    const entry = `usr/share/applications/${desktopId}.desktop`;
    await NodeFSP.mkdir(NodePath.dirname(NodePath.join(app, icon)), { recursive: true });
    await NodeFSP.mkdir(NodePath.dirname(NodePath.join(app, entry)), { recursive: true });
    await NodeFSP.copyFile(
      NodePath.join(server, "client/icon-512x512.png"),
      NodePath.join(app, icon),
    );
    const desktopEntry = await NodeFSP.readFile(
      NodePath.join(desktop, "native/t3-code.desktop"),
      "utf8",
    );
    await NodeFSP.writeFile(
      NodePath.join(app, entry),
      desktopEntry
        .replace(/^Name=.*$/m, `Name=${product}`)
        .replace(
          /^Exec=.*$/m,
          linuxUpdate
            ? `Exec=${binaryName} %U\nMimeType=x-scheme-handler/t3code;x-scheme-handler/t3code-dev;`
            : `Exec=${binaryName}`,
        )
        .replace(/^Icon=.*$/m, `Icon=${desktopId}`)
        .replace(/^StartupWMClass=.*$/m, `StartupWMClass=${desktopId}`),
    );
    if (buildDeb) {
      await buildRuneDeb({
        root: app,
        tmp,
        artifact: NodePath.join(published, artifactName("deb")),
        architecture: arch === "x64" ? "amd64" : arch,
        version,
        product,
        desktopId,
        linuxUpdate,
        desktop,
        license: NodePath.join(repoRoot, "LICENSE"),
      });
    }
    if (unpacked || formats.includes("AppImage")) {
      if (linuxUpdate) await writeReleaseSource("AppImage");
      if (buildDeb) await NodeFSP.rm(NodePath.join(app, "DEBIAN"), { recursive: true });
      await Effect.runPromise(
        bundleLinuxLibatomic({ repoRoot, contentDir: resources, executablePath: bundledNode }).pipe(
          Effect.scoped,
          Effect.provide(NodeServices.layer),
        ),
      );
      if (linuxUpdate)
        await NodeFSP.symlink(NodePath.relative(app, executable), NodePath.join(app, binaryName));
      for (const [name, target] of [
        ["AppRun", NodePath.relative(app, executable)],
        [`${desktopId}.desktop`, entry],
        [`${desktopId}.png`, icon],
        [".DirIcon", `${desktopId}.png`],
      ]) {
        await NodeFSP.symlink(target, NodePath.join(app, name));
      }
      if (!unpacked)
        run(
          "appimagetool",
          [
            "--no-appstream",
            "--runtime-file",
            await writeAppImageRuntime(tmp),
            app,
            NodePath.join(published, artifactName("AppImage")),
          ],
          {
            env: { ...process.env, ARCH: cpu },
          },
        );
    }
  }
  if (unpacked && keepStage) {
    if (mac) {
      await NodeFSP.mkdir(unpackedDir);
      run("ditto", [app, unpackedApp]);
    } else {
      await NodeFSP.cp(app, unpackedApp, {
        recursive: true,
        verbatimSymlinks: true,
        mode: NodeFSP.constants.COPYFILE_FICLONE,
      });
    }
  }
  const names = unpacked ? [unpackedName] : formats.map(artifactName);
  if (linuxUpdate) {
    const { writeLinuxUpdateFiles } = await import("./lib/rune-update.mjs");
    names.push(
      ...(await writeLinuxUpdateFiles(published, {
        version,
        channel,
        arch,
        image: artifactName("AppImage"),
        deb: artifactName("deb"),
      })),
    );
  } else if (updateFeed && win) {
    const { writeWindowsUpdateFiles } = await import("./lib/rune-update.mjs");
    names.push(
      ...(await writeWindowsUpdateFiles(published, {
        version,
        channel,
        arch,
        installer: artifactName("nsis"),
      })),
    );
  } else if (updateFeed) {
    const { writeMacUpdateFiles } = await import("./lib/rune-update.mjs");
    names.push(
      ...(await writeMacUpdateFiles(published, {
        version,
        channel,
        arch,
        zip: artifactName("zip"),
        dmg: formats.includes("dmg") ? artifactName("dmg") : undefined,
      })),
    );
  }
  const publish = Effect.promise(() =>
    unpacked
      ? publishUnpackedRuneApp(published, output, unpackedName)
      : publishRuneArtifacts(published, output, names, { requireFresh: updateFeed }),
  );
  // Separate checkouts can publish to the same output directory.
  const [checkoutDir, outputDir] = await Promise.all(
    [repoRoot, output].map((path) => NodeFSP.stat(path, { bigint: true })),
  );
  const checkoutOutput =
    checkoutDir.ino && outputDir.ino
      ? checkoutDir.dev === outputDir.dev && checkoutDir.ino === outputDir.ino
      : (await NodeFSP.realpath(output)) === (await NodeFSP.realpath(repoRoot));
  await Effect.runPromise(
    (checkoutOutput ? publish : withDesktopBuildLock(output, publish)).pipe(
      Effect.provide(NodeServices.layer),
    ),
  );
  for (const name of names) console.log(NodePath.join(output, name));
}

await Effect.runPromise(
  withDesktopBuildLock(repoRoot, Effect.promise(buildRunePackage)).pipe(
    Effect.provide(NodeServices.layer),
  ),
);
