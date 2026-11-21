// @effect-diagnostics nodeBuiltinImport:off - Node's incremental SHA-256 hashes WSL archives without buffering them in memory.
import * as NodeCrypto from "node:crypto";
import { resolveSpawnCommand } from "@t3tools/shared/shell";
import desktopPackageJson from "../apps/desktop/package.json" with { type: "json" };
import {
  BRAND_ASSET_PATHS,
  resolveWebAssetBrandForChannel,
  type WebAssetBrand,
} from "./lib/brand-assets.ts";
import * as Config from "effect/Config";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import { ChildProcess, ChildProcessSpawner } from "effect/process";

export const BuildPlatform = Schema.Literals(["mac", "linux", "win"]);
export const BuildArch = Schema.Literals(["arm64", "x64", "universal"]);

export const StageWorkspaceConfig = Schema.Struct({
  supportedArchitectures: Schema.Struct({
    os: Schema.Array(Schema.String),
    cpu: Schema.Array(Schema.String),
    libc: Schema.optional(Schema.Array(Schema.String)),
  }),
  // pnpm 11 reads build approvals only from pnpm-workspace.yaml; omission fails staged lifecycle installs.
  allowBuilds: Schema.optional(Schema.Record(Schema.String, Schema.Boolean)),
  patchedDependencies: Schema.optional(Schema.Record(Schema.String, Schema.String)),
  overrides: Schema.optional(Schema.Record(Schema.String, Schema.String)),
  packageExtensions: Schema.optional(
    Schema.Record(
      Schema.String,
      Schema.Struct({
        peerDependenciesMeta: Schema.Record(
          Schema.String,
          Schema.Struct({ optional: Schema.Boolean }),
        ),
      }),
    ),
  ),
  nodeLinker: Schema.optional(Schema.Literals(["hoisted"])),
});
export type StageWorkspaceConfig = typeof StageWorkspaceConfig.Type;

// TypeScript is a typings-only peer; pnpm auto-installation would ship a compiler.
const STAGE_PACKAGE_EXTENSIONS = {
  "electron-webauthn": { peerDependenciesMeta: { typescript: { optional: true } } },
  "@electron-webauthn/macos": { peerDependenciesMeta: { typescript: { optional: true } } },
} as const;

export interface DesktopBuildIconAssets {
  readonly macIconPng: string;
  readonly linuxIconPng: string;
  readonly windowsIconIco: string;
}

export function resolveResourceMonitorRustTargets(
  platform: typeof BuildPlatform.Type,
  arch: typeof BuildArch.Type,
): ReadonlyArray<string> {
  if (platform === "mac") {
    if (arch === "universal") {
      return ["aarch64-apple-darwin", "x86_64-apple-darwin"];
    }
    return [arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin"];
  }
  if (platform === "linux") {
    return [arch === "arm64" ? "aarch64-unknown-linux-gnu" : "x86_64-unknown-linux-gnu"];
  }
  return [arch === "arm64" ? "aarch64-pc-windows-msvc" : "x86_64-pc-windows-msvc"];
}

export function resourceMonitorExecutableName(platform: typeof BuildPlatform.Type): string {
  return platform === "win" ? "t3-resource-monitor.exe" : "t3-resource-monitor";
}

export class BuildCommandFailedError extends Schema.TaggedError<BuildCommandFailedError>()(
  "BuildCommandFailedError",
  {
    command: Schema.String,
    exitCode: Schema.Int,
    stdoutTail: Schema.optionalKey(Schema.String),
    stderrTail: Schema.optionalKey(Schema.String),
  },
) {
  override get message(): string {
    const outputSections = [
      `Command: ${this.command}`,
      formatOutputSection("stdout", this.stdoutTail ?? ""),
      formatOutputSection("stderr", this.stderrTail ?? ""),
    ].filter((section): section is string => section !== undefined);
    const outputSuffix = outputSections.length > 0 ? `\n\n${outputSections.join("\n\n")}` : "";
    return `Command exited with non-zero exit code (${this.exitCode})${outputSuffix}`;
  }
}

export class ResourceMonitorBuildOutputMissingError extends Schema.TaggedError<ResourceMonitorBuildOutputMissingError>()(
  "ResourceMonitorBuildOutputMissingError",
  {
    binaryPath: Schema.String,
    rustTarget: Schema.String,
    platform: BuildPlatform,
    arch: BuildArch,
  },
) {
  override get message(): string {
    return `Resource monitor build for ${this.rustTarget} did not produce ${this.binaryPath}.`;
  }
}

const desktopIconPlatformNames = {
  mac: "macOS",
  linux: "Linux",
  win: "Windows",
} satisfies Record<typeof BuildPlatform.Type, string>;

export class DesktopIconSourceMissingError extends Schema.TaggedError<DesktopIconSourceMissingError>()(
  "DesktopIconSourceMissingError",
  {
    platform: BuildPlatform,
    sourcePath: Schema.String,
  },
) {
  override get message(): string {
    return `Desktop ${desktopIconPlatformNames[this.platform]} icon source is missing at ${this.sourcePath}`;
  }
}

export class DesktopDmgBackgroundSourceMissingError extends Schema.TaggedError<DesktopDmgBackgroundSourceMissingError>()(
  "DesktopDmgBackgroundSourceMissingError",
  {
    channel: Schema.Literals(["latest", "nightly"]),
    sourcePath: Schema.String,
  },
) {
  override get message(): string {
    return `Desktop ${this.channel} DMG background source is missing at ${this.sourcePath}`;
  }
}

export class WslRuntimeArchiveMissingError extends Schema.TaggedError<WslRuntimeArchiveMissingError>()(
  "WslRuntimeArchiveMissingError",
  {
    archivePath: Schema.String,
  },
) {
  override get message(): string {
    return `WSL runtime archive not found at ${this.archivePath}.`;
  }
}

const WindowsPackagedPayloadValidationReason = Schema.Literals([
  "packaged-app-missing",
  "sidecar-missing",
  "sidecar-invalid",
  "unpacked-native-missing",
  "resource-monitor-missing",
  "wsl-runtime-missing",
  "wsl-runtime-invalid",
  "file-limit-exceeded",
]);

export class WindowsPackagedPayloadValidationError extends Schema.TaggedError<WindowsPackagedPayloadValidationError>()(
  "WindowsPackagedPayloadValidationError",
  {
    reason: WindowsPackagedPayloadValidationReason,
    packagedAppDir: Schema.String,
    missingFiles: Schema.optionalKey(Schema.Array(Schema.String)),
    fileCount: Schema.optionalKey(Schema.Int),
    fileLimit: Schema.optionalKey(Schema.Int),
    cause: Schema.optionalKey(Schema.Defect()),
  },
) {
  override get message(): string {
    if (this.reason === "file-limit-exceeded") {
      return `Windows packaged payload contains ${String(this.fileCount)} files; expected at most ${String(this.fileLimit)}.`;
    }
    if (this.reason === "unpacked-native-missing") {
      return `Windows server sidecar is missing ${String(this.missingFiles?.length ?? 0)} unpacked native files.`;
    }
    if (this.reason === "resource-monitor-missing") {
      return "Windows packaged payload is missing the resource monitor executable.";
    }
    if (this.reason === "wsl-runtime-missing") {
      return "Windows packaged payload is missing the WSL runtime archive or SHA-256 sidecar.";
    }
    if (this.reason === "wsl-runtime-invalid") {
      return "Windows packaged payload contains an invalid WSL runtime archive.";
    }
    if (this.reason === "sidecar-invalid") {
      return "Windows packaged payload contains an invalid server.asar sidecar.";
    }
    if (this.reason === "sidecar-missing") {
      return "Windows packaged payload is missing resources/server.asar.";
    }
    return `Windows packaged application directory was not found at ${this.packagedAppDir}.`;
  }
}

const collectStreamAsString = <E>(stream: Stream.Stream<Uint8Array, E>): Effect.Effect<string, E> =>
  stream.pipe(
    Stream.decodeText(),
    Stream.runFold(
      () => "",
      (acc, chunk) => acc + chunk,
    ),
  );

const COMMAND_OUTPUT_TAIL_LENGTH = 20_000;

function appendOutputTail(acc: string, chunk: string): string {
  const next = acc + chunk;
  return next.length > COMMAND_OUTPUT_TAIL_LENGTH ? next.slice(-COMMAND_OUTPUT_TAIL_LENGTH) : next;
}

function formatOutputSection(label: string, output: string): string | undefined {
  const trimmed = output.trim();
  if (!trimmed) return undefined;
  return `${label} tail:\n${trimmed}`;
}

const collectCommandStream = <E>(
  stream: Stream.Stream<Uint8Array, E>,
  output: NodeJS.WriteStream,
  verbose: boolean,
): Effect.Effect<string, E> =>
  stream.pipe(
    Stream.decodeText(),
    Stream.runFoldEffect(
      () => "",
      (acc, chunk) =>
        Effect.as(
          verbose ? Effect.sync(() => output.write(chunk)) : Effect.void,
          appendOutputTail(acc, chunk),
        ),
    ),
  );

export const spawnAndCollectOutput = Effect.fn("spawnAndCollectOutput")(function* (
  command: ChildProcess.Command,
) {
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const child = yield* spawner.spawn(command);

  const [stdout, stderr, exitCode] = yield* Effect.all(
    [
      collectStreamAsString(child.stdout),
      collectStreamAsString(child.stderr),
      child.exitCode.pipe(Effect.map(Number)),
    ],
    { concurrency: "unbounded" },
  );

  return { stdout, stderr, exitCode } as const;
});

export const STAGE_INSTALL_ARGS = ["install", "--prod"] as const;
export const WSL_RUNTIME_ARCHIVE_NAME = "wsl-runtime.tar.gz";
export const WSL_RUNTIME_ARCHIVE_HASH_NAME = `${WSL_RUNTIME_ARCHIVE_NAME}.sha256`;

export function resolveFffNativeDependencies(
  platform: typeof BuildPlatform.Type,
  arch: typeof BuildArch.Type,
  version: string,
): Record<string, string> {
  const architectures = arch === "universal" ? (["arm64", "x64"] as const) : [arch];

  if (platform === "mac") {
    return Object.fromEntries(
      architectures.map((architecture) => [`@ff-labs/fff-bin-darwin-${architecture}`, version]),
    );
  }

  if (platform === "win") {
    return Object.fromEntries(
      architectures.map((architecture) => [`@ff-labs/fff-bin-win32-${architecture}`, version]),
    );
  }

  return Object.fromEntries(
    architectures.flatMap((architecture) =>
      ["gnu", "musl"].map((libc) => [`@ff-labs/fff-bin-linux-${architecture}-${libc}`, version]),
    ),
  );
}

export function createStageWorkspaceConfig(input: {
  readonly platform: typeof BuildPlatform.Type;
  readonly arch: typeof BuildArch.Type;
  readonly allowBuilds?: Record<string, boolean>;
  readonly patchedDependencies?: Record<string, string>;
  readonly overrides?: Record<string, string>;
}): StageWorkspaceConfig {
  const { platform, arch, allowBuilds, patchedDependencies, overrides } = input;
  const hostOs = platform === "mac" ? "darwin" : platform === "win" ? "win32" : "linux";
  const hostCpu = arch === "universal" ? ["arm64", "x64"] : [arch];
  // Linux AppImages require glibc native optional dependencies; tell pnpm explicitly.
  const supportedArchitectures =
    platform === "linux"
      ? {
          os: [hostOs],
          cpu: hostCpu,
          libc: ["glibc"],
        }
      : {
          os: [hostOs],
          cpu: hostCpu,
        };

  return {
    supportedArchitectures,
    ...(allowBuilds && Object.keys(allowBuilds).length > 0 ? { allowBuilds } : {}),
    ...(patchedDependencies && Object.keys(patchedDependencies).length > 0
      ? { patchedDependencies }
      : {}),
    ...(overrides && Object.keys(overrides).length > 0 ? { overrides } : {}),
    packageExtensions: STAGE_PACKAGE_EXTENSIONS,
  };
}

export function createStagePatchedDependencies(
  patchedDependencies: Record<string, string>,
  dependencies: Record<string, unknown>,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(patchedDependencies).filter(([patchKey]) =>
      Object.hasOwn(dependencies, getPatchedDependencyPackageName(patchKey)),
    ),
  );
}

function getPatchedDependencyPackageName(patchKey: string): string {
  const versionSeparator = patchKey.lastIndexOf("@");
  return versionSeparator > 0 ? patchKey.slice(0, versionSeparator) : patchKey;
}

export const AzureTrustedSigningOptionsConfig = Config.all({
  publisherName: Config.String("AZURE_TRUSTED_SIGNING_PUBLISHER_NAME"),
  endpoint: Config.String("AZURE_TRUSTED_SIGNING_ENDPOINT"),
  certificateProfileName: Config.String("AZURE_TRUSTED_SIGNING_CERTIFICATE_PROFILE_NAME"),
  codeSigningAccountName: Config.String("AZURE_TRUSTED_SIGNING_ACCOUNT_NAME"),
  fileDigest: Config.String("AZURE_TRUSTED_SIGNING_FILE_DIGEST").pipe(Config.withDefault("SHA256")),
  timestampDigest: Config.String("AZURE_TRUSTED_SIGNING_TIMESTAMP_DIGEST").pipe(
    Config.withDefault("SHA256"),
  ),
  timestampRfc3161: Config.String("AZURE_TRUSTED_SIGNING_TIMESTAMP_RFC3161").pipe(
    Config.withDefault("http://timestamp.acs.microsoft.com"),
  ),
});

export const runCommand = Effect.fn("runCommand")(function* (
  command: ChildProcess.Command,
  options: {
    readonly label: string;
    readonly verbose: boolean;
  },
) {
  const commandSpawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const child = yield* commandSpawner.spawn(command);
  const [stdout, stderr, exitCode] = yield* Effect.all(
    [
      collectCommandStream(child.stdout, process.stdout, options.verbose),
      collectCommandStream(child.stderr, process.stderr, options.verbose),
      child.exitCode.pipe(Effect.map(Number)),
    ],
    { concurrency: "unbounded" },
  );

  if (exitCode !== 0) {
    return yield* new BuildCommandFailedError({
      command: options.label,
      exitCode,
      ...(stdout.trim() ? { stdoutTail: stdout } : {}),
      ...(stderr.trim() ? { stderrTail: stderr } : {}),
    });
  }
});

export class DesktopBuildAlreadyRunningError extends Schema.TaggedError<DesktopBuildAlreadyRunningError>()(
  "DesktopBuildAlreadyRunningError",
  { lockPath: Schema.String },
) {
  override get message(): string {
    return `Another desktop build owns '${this.lockPath}'. Wait for it to finish. If a build was killed, confirm no desktop build is running, inspect and restore any changed release manifests, then remove this lock.`;
  }
}

export const withDesktopBuildLock = Effect.fn("withDesktopBuildLock")(function* <A, E, R>(
  repoRoot: string,
  build: Effect.Effect<A, E, R>,
) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const lockPath = path.join(repoRoot, ".desktop-build.lock");
  return yield* Effect.acquireUseRelease(
    fs
      .makeDirectory(lockPath)
      .pipe(
        Effect.mapError((error) =>
          error.reason._tag === "AlreadyExists"
            ? new DesktopBuildAlreadyRunningError({ lockPath })
            : error,
        ),
      ),
    // Stop packaging children and finish stage cleanup before another build can enter.
    () => build.pipe(Effect.scoped),
    (_, exit) =>
      fs
        .remove(lockPath, { recursive: true })
        .pipe(
          Effect.catch((error) =>
            Exit.isFailure(exit) ? Effect.logError(error) : Effect.fail(error),
          ),
        ),
  );
});

export const stageResourceMonitor = Effect.fn("stageResourceMonitor")(function* (input: {
  readonly repoRoot: string;
  readonly stageResourcesDir: string;
  readonly platform: typeof BuildPlatform.Type;
  readonly arch: typeof BuildArch.Type;
  readonly rustTarget?: string;
  readonly verbose: boolean;
}) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const manifestPath = path.join(input.repoRoot, "native/resource-monitor/Cargo.toml");
  const executableName = resourceMonitorExecutableName(input.platform);
  const rustTargets = input.rustTarget
    ? [input.rustTarget]
    : resolveResourceMonitorRustTargets(input.platform, input.arch);
  const reuseResourceMonitor = yield* Config.Boolean("T3CODE_DESKTOP_REUSE_RESOURCE_MONITOR").pipe(
    Config.withDefault(false),
  );
  const builtBinaries: string[] = [];

  for (const rustTarget of rustTargets) {
    if (!reuseResourceMonitor) {
      const spawnCommand = yield* resolveSpawnCommand("cargo", [
        "build",
        "--locked",
        "--release",
        "--manifest-path",
        manifestPath,
        "--target",
        rustTarget,
      ]);
      yield* runCommand(
        ChildProcess.make(spawnCommand.command, spawnCommand.args, {
          cwd: input.repoRoot,
          shell: spawnCommand.shell,
        }),
        {
          label: `cargo build resource monitor (${rustTarget})`,
          verbose: input.verbose,
        },
      );
    }

    const binaryPath = path.join(
      input.repoRoot,
      "native/resource-monitor/target",
      rustTarget,
      "release",
      executableName,
    );
    if (!(yield* fs.exists(binaryPath))) {
      return yield* new ResourceMonitorBuildOutputMissingError({
        binaryPath,
        rustTarget,
        platform: input.platform,
        arch: input.arch,
      });
    }
    if (reuseResourceMonitor) {
      yield* Effect.log(`[desktop-artifact] Reusing cached resource monitor (${rustTarget}).`);
    }
    builtBinaries.push(binaryPath);
  }

  const destinationDirectory = path.join(input.stageResourcesDir, "resource-monitor");
  const destinationPath = path.join(destinationDirectory, executableName);
  yield* fs.remove(destinationDirectory, { recursive: true, force: true }).pipe(Effect.ignore);
  yield* fs.makeDirectory(destinationDirectory, { recursive: true });

  if (builtBinaries.length === 1) {
    yield* fs.copyFile(builtBinaries[0]!, destinationPath);
  } else {
    yield* runCommand(
      ChildProcess.make("lipo", ["-create", ...builtBinaries, "-output", destinationPath]),
      {
        label: "lipo resource monitor universal binary",
        verbose: input.verbose,
      },
    );
  }

  if (input.platform !== "win") {
    yield* fs.chmod(destinationPath, 0o755);
  }
});

const generateMacIconSet = Effect.fn("build-desktop-artifact.generateMacIconSet")(function* (
  sourcePng: string,
  targetIcns: string,
  tmpRoot: string,
  path: Path.Path,
  verbose: boolean,
) {
  const fs = yield* FileSystem.FileSystem;
  const iconsetDir = path.join(tmpRoot, "icon.iconset");
  yield* fs.makeDirectory(iconsetDir, { recursive: true });

  const iconSizes = [16, 32, 128, 256, 512] as const;
  for (const size of iconSizes) {
    yield* runCommand(
      ChildProcess.make(
        {},
      )`sips -z ${size} ${size} ${sourcePng} --out ${path.join(iconsetDir, `icon_${size}x${size}.png`)}`,
      { label: `sips icon ${size}x${size}`, verbose },
    );

    const retinaSize = size * 2;
    yield* runCommand(
      ChildProcess.make(
        {},
      )`sips -z ${retinaSize} ${retinaSize} ${sourcePng} --out ${path.join(iconsetDir, `icon_${size}x${size}@2x.png`)}`,
      { label: `sips icon ${size}x${size}@2x`, verbose },
    );
  }

  yield* runCommand(ChildProcess.make({})`iconutil -c icns ${iconsetDir} -o ${targetIcns}`, {
    label: "iconutil icns",
    verbose,
  });
});

export const stageMacIcons = Effect.fn("build-desktop-artifact.stageMacIcons")(function* (
  stageResourcesDir: string,
  sourcePng: string,
  verbose: boolean,
) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  if (!(yield* fs.exists(sourcePng))) {
    return yield* new DesktopIconSourceMissingError({
      platform: "mac",
      sourcePath: sourcePng,
    });
  }

  const tmpRoot = yield* fs.makeTempDirectoryScoped({
    prefix: "t3code-icon-build-",
  });

  const iconPngPath = path.join(stageResourcesDir, "icon.png");
  const iconIcnsPath = path.join(stageResourcesDir, "icon.icns");

  yield* runCommand(ChildProcess.make({})`sips -z 512 512 ${sourcePng} --out ${iconPngPath}`, {
    label: "sips mac icon",
    verbose,
  });

  yield* generateMacIconSet(sourcePng, iconIcnsPath, tmpRoot, path, verbose);
});

export const stageDesktopDmgBackground = Effect.fn("stageDesktopDmgBackground")(function* (
  stageResourcesDir: string,
  channel: "latest" | "nightly",
  verbose: boolean,
) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const sourcePath = path.join(stageResourcesDir, "dmg", `dmg-background-${channel}.svg`);
  if (!(yield* fs.exists(sourcePath))) {
    return yield* new DesktopDmgBackgroundSourceMissingError({ channel, sourcePath });
  }

  for (const output of [
    { suffix: "", width: 640, height: 432 },
    { suffix: "@2x", width: 1280, height: 864 },
  ] as const) {
    const targetPath = path.join(
      stageResourcesDir,
      "dmg",
      `dmg-background-${channel}${output.suffix}.png`,
    );
    yield* runCommand(
      ChildProcess.make(
        {},
      )`sips -s format png -z ${output.height} ${output.width} ${sourcePath} --out ${targetPath}`,
      {
        label: `sips ${channel} DMG background${output.suffix || "@1x"}`,
        verbose,
      },
    );
  }
});

export function resolveDesktopUpdateChannel(version: string): "latest" | "nightly" {
  return /-nightly\.\d{8}\.\d+$/.test(version) ? "nightly" : "latest";
}

export function resolveDesktopWebAssetBrand(version: string): WebAssetBrand {
  return resolveWebAssetBrandForChannel(resolveDesktopUpdateChannel(version));
}

export function resolveDesktopBuildIconAssets(version: string): DesktopBuildIconAssets {
  if (resolveDesktopUpdateChannel(version) === "nightly") {
    return {
      macIconPng: BRAND_ASSET_PATHS.nightlyMacIconPng,
      linuxIconPng: BRAND_ASSET_PATHS.nightlyLinuxIconPng,
      windowsIconIco: BRAND_ASSET_PATHS.nightlyWindowsIconIco,
    };
  }

  return {
    macIconPng: BRAND_ASSET_PATHS.productionMacIconPng,
    linuxIconPng: BRAND_ASSET_PATHS.productionLinuxIconPng,
    windowsIconIco: BRAND_ASSET_PATHS.productionWindowsIconIco,
  };
}

export function resolveDesktopProductName(version: string): string {
  return resolveDesktopUpdateChannel(version) === "nightly"
    ? "T3 Code (Nightly)"
    : (desktopPackageJson.productName ?? "T3 Code");
}

export function desktopDmgLayout(version: string) {
  return {
    // Finder caches DMG backgrounds by volume name; give each installer its own.
    title: `${resolveDesktopProductName(version)} ${version} Installer`,
    background: `dmg/dmg-background-${resolveDesktopUpdateChannel(version)}.png`,
    window: {
      width: 640,
      // DMG bounds include Finder's 32px title bar; keep that artwork strip clear.
      height: 432,
    },
    contents: [
      { x: 166, y: 214, type: "file" as const },
      { x: 474, y: 214, type: "link" as const, path: "/Applications" },
    ],
    iconSize: 120,
    iconTextSize: 12,
  };
}

// The digest verifies extraction and names the cache; hash the exact archive bytes that ship.
export const stageWslRuntimeArchive = Effect.fn("stageWslRuntimeArchive")(function* (input: {
  readonly sourceArchivePath: string;
  readonly archivePath: string;
  readonly hashPath: string;
}) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const sourceExists = yield* fs
    .exists(input.sourceArchivePath)
    .pipe(Effect.orElseSucceed(() => false));
  if (!sourceExists) {
    return yield* new WslRuntimeArchiveMissingError({ archivePath: input.sourceArchivePath });
  }
  yield* fs.makeDirectory(path.dirname(input.archivePath), { recursive: true });
  yield* fs.copyFile(input.sourceArchivePath, input.archivePath);
  const hash = NodeCrypto.createHash("sha256");
  yield* fs
    .stream(input.archivePath)
    .pipe(Stream.runForEach((chunk) => Effect.sync(() => hash.update(chunk))));
  const digest = hash.digest("hex");
  yield* fs.writeFileString(input.hashPath, `${digest}\n`);
  yield* Effect.log(
    `[desktop-artifact] Staged WSL runtime archive at ${input.archivePath} (${digest}).`,
  );
});

// Match cliArchiveStem without its circular import. WSL uses the Windows host's architecture.
export const wslRuntimeArchiveStem = (version: string, arch: typeof BuildArch.Type): string =>
  `t3-${version}-linux-${arch}`;

export const parseWslRuntimeArchiveMembers = (listing: string): ReadonlyArray<string> =>
  listing
    .split(/\r?\n/)
    .map((member) => member.replace(/^\.\//, "").replace(/\/$/, ""))
    .filter((member) => member.length > 0);

export const validatePackagedWslRuntime = Effect.fn("desktopArtifact.validatePackagedWslRuntime")(
  function* (input: {
    readonly packagedAppDir: string;
    readonly appVersion: string;
    readonly targetArch: typeof BuildArch.Type;
  }) {
    const fs = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const { packagedAppDir } = input;
    const resourcesDir = path.join(packagedAppDir, "resources");
    const wslArchivePath = path.join(resourcesDir, WSL_RUNTIME_ARCHIVE_NAME);
    const wslArchiveHashPath = path.join(resourcesDir, WSL_RUNTIME_ARCHIVE_HASH_NAME);
    const invalidWslRuntime = (cause: unknown) =>
      new WindowsPackagedPayloadValidationError({
        reason: "wsl-runtime-invalid",
        packagedAppDir,
        cause,
      });
    const recordedHash = yield* fs
      .readFileString(wslArchiveHashPath)
      .pipe(Effect.mapError(invalidWslRuntime));
    const expectedHash = recordedHash.trim().toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(expectedHash)) {
      return yield* invalidWslRuntime(new Error("invalid WSL runtime SHA-256 sidecar"));
    }
    const archiveHash = NodeCrypto.createHash("sha256");
    yield* fs.stream(wslArchivePath).pipe(
      Stream.runForEach((chunk) => Effect.sync(() => archiveHash.update(chunk))),
      Effect.mapError(invalidWslRuntime),
    );
    const actualHash = archiveHash.digest("hex");
    if (actualHash !== expectedHash) {
      return yield* invalidWslRuntime(
        new Error(`WSL runtime SHA-256 mismatch: expected ${expectedHash}, got ${actualHash}`),
      );
    }

    const listing = yield* spawnAndCollectOutput(
      ChildProcess.make("tar", ["-tzf", WSL_RUNTIME_ARCHIVE_NAME], {
        cwd: resourcesDir,
        stdin: "ignore",
        stdout: "pipe",
        stderr: "pipe",
      }),
    ).pipe(Effect.mapError(invalidWslRuntime));
    if (listing.exitCode !== 0) {
      return yield* invalidWslRuntime(
        new Error(`tar could not list WSL runtime archive: ${listing.stderr.trim()}`),
      );
    }
    const members = parseWslRuntimeArchiveMembers(listing.stdout);
    // The WSL installer finds t3 under the archive's single stem-named directory.
    const stem = wslRuntimeArchiveStem(input.appVersion, input.targetArch);
    const topLevel = new Set(members.map((member) => member.split("/")[0]));
    if (topLevel.size !== 1 || !topLevel.has(stem)) {
      return yield* invalidWslRuntime(
        new Error(
          `WSL runtime archive must contain a single top-level directory ${stem}, found ${[...topLevel].join(", ") || "nothing"}`,
        ),
      );
    }
    const requiredMembers = [`${stem}/t3`, `${stem}/client`, `${stem}/node_modules`];
    const missingMembers = requiredMembers.filter((member) => !members.includes(member));
    const ptyCandidates = [
      `${stem}/node_modules/node-pty/build/Release/pty.node`,
      `${stem}/node_modules/node-pty/prebuilds/linux-${input.targetArch}/pty.node`,
    ];
    if (!ptyCandidates.some((member) => members.includes(member))) {
      missingMembers.push(...ptyCandidates);
    }
    if (missingMembers.length > 0) {
      return yield* new WindowsPackagedPayloadValidationError({
        reason: "wsl-runtime-invalid",
        packagedAppDir,
        missingFiles: missingMembers,
        cause: new Error("WSL runtime archive is not a Linux CLI release archive"),
      });
    }
    const bundleEntry = members.find((member) => member.endsWith("/bin.mjs"));
    if (bundleEntry !== undefined) {
      return yield* invalidWslRuntime(
        new Error(`WSL runtime archive contains a server bundle entry ${bundleEntry}`),
      );
    }
  },
);
