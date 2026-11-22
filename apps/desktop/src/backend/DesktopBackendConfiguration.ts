import * as NodeOS from "node:os";

import { parsePersistedServerObservabilitySettings } from "@t3tools/shared/serverSettings";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as SecureRandom from "effect/crypto/SecureRandom";
import * as Effect from "effect/Effect";
import * as Hex from "effect/encoding/Hex";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as PlatformError from "effect/PlatformError";
import * as Schema from "effect/Schema";

import serverPackageJson from "../../../server/package.json" with { type: "json" };

import { backendChildEnvPatch } from "./DesktopBackendProcess.ts";
import type { DesktopBackendStartConfig } from "./DesktopBackendProcess.ts";
import * as DesktopEnvironment from "../app/DesktopEnvironment.ts";
import * as DesktopWslEnvironment from "../wsl/DesktopWslEnvironment.ts";
import * as DesktopWslServerTree from "../wsl/DesktopWslServerTree.ts";

export class DesktopBackendObservabilitySettingsReadError extends Schema.TaggedError<DesktopBackendObservabilitySettingsReadError>()(
  "DesktopBackendObservabilitySettingsReadError",
  {
    settingsPath: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to read persisted backend observability settings at ${this.settingsPath}.`;
  }
}

interface BackendObservabilitySettings {
  readonly otlpTracesUrl: Option.Option<string>;
  readonly otlpMetricsUrl: Option.Option<string>;
  readonly otlpLogsUrl: Option.Option<string>;
}

const emptyBackendObservabilitySettings: BackendObservabilitySettings = {
  otlpTracesUrl: Option.none(),
  otlpMetricsUrl: Option.none(),
  otlpLogsUrl: Option.none(),
};

// Windows forwards these variables into WSL only through WSLENV.
const WSL_FORWARDED_ENV_NAMES = [
  "OPENAI_API_KEY",
  "ANTHROPIC_API_KEY",
  "T3CODE_TELEMETRY_ENABLED",
  "T3CODE_GITHUB_APP_CLIENT_ID",
  // Otherwise the WSL server keeps exporting to endpoints from the bootstrap.
  "T3CODE_OTEL_SDK_DISABLED",
  "OTEL_SDK_DISABLED",
  "T3CODE_OTLP_HEADERS",
  "T3CODE_OTLP_PROTOCOL",
  // Untranslated T3 URLs must override bootstrap endpoints inside WSL, as they do on Windows.
  "T3CODE_OTLP_TRACES_URL",
  "T3CODE_OTLP_METRICS_URL",
  "T3CODE_OTLP_LOGS_URL",
  "OTEL_EXPORTER_OTLP_ENDPOINT",
  "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
  "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
  "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
  "OTEL_EXPORTER_OTLP_HEADERS",
  "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
  "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
  "OTEL_EXPORTER_OTLP_LOGS_HEADERS",
  "OTEL_EXPORTER_OTLP_PROTOCOL",
  "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
  "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
  "OTEL_EXPORTER_OTLP_LOGS_PROTOCOL",
  "OTEL_TRACES_EXPORTER",
  "OTEL_METRICS_EXPORTER",
  "OTEL_LOGS_EXPORTER",
  "OTEL_RESOURCE_ATTRIBUTES",
] as const;

const WSL_SERVER_SYSTEM_PATH = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

const nodeBinDirOf = (nodePath: string): string => {
  const lastSlash = nodePath.lastIndexOf("/");
  return lastSlash > 0 ? nodePath.slice(0, lastSlash) : "/usr/bin";
};

const getWslEnvEntryName = (entry: string): string => {
  const slashIndex = entry.indexOf("/");
  return slashIndex === -1 ? entry : entry.slice(0, slashIndex);
};

const mergeWslEnv = (
  existingWslEnv: string | undefined,
  forwardedEnvNames: ReadonlyArray<string>,
): string | undefined => {
  const existing = existingWslEnv?.trim() ?? "";

  const seenNames = new Set(
    existing
      .split(":")
      .map((entry) => getWslEnvEntryName(entry.trim()))
      .filter((name) => name.length > 0),
  );

  const additions = forwardedEnvNames.filter((name) => !seenNames.has(name));

  // Preserve empty segments and duplicates in the user's WSLENV; only append required variables.
  const parts = [existing, ...additions].filter((part) => part.length > 0);
  return parts.length > 0 ? parts.join(":") : undefined;
};

const logBackendObservabilitySettingsReadFailure = (
  settingsPath: string,
  cause: PlatformError.PlatformError,
) => {
  const error = new DesktopBackendObservabilitySettingsReadError({ settingsPath, cause });
  return Effect.logWarning(error).pipe(
    Effect.annotateLogs({
      component: "desktop-backend-configuration",
      error,
    }),
  );
};

const readPersistedBackendObservabilitySettings = Effect.gen(function* () {
  const fileSystem = yield* FileSystem.FileSystem;
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const raw = yield* fileSystem.readFileString(environment.serverSettingsPath).pipe(
    Effect.asSome,
    Effect.catchTags({
      PlatformError: (cause) =>
        cause.reason._tag === "NotFound"
          ? Effect.succeedNone
          : logBackendObservabilitySettingsReadFailure(environment.serverSettingsPath, cause).pipe(
              Effect.as(Option.none()),
            ),
    }),
  );
  if (Option.isNone(raw)) {
    return emptyBackendObservabilitySettings;
  }

  const parsed = parsePersistedServerObservabilitySettings(raw.value);
  return {
    otlpTracesUrl: Option.fromNullishOr(parsed.otlpTracesUrl),
    otlpMetricsUrl: Option.fromNullishOr(parsed.otlpMetricsUrl),
    otlpLogsUrl: Option.fromNullishOr(parsed.otlpLogsUrl),
  };
});

export const readBackendObservabilitySettings = Effect.gen(function* () {
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const persisted = yield* readPersistedBackendObservabilitySettings;
  return {
    otlpTracesUrl: Option.orElse(environment.otlpTracesUrl, () => persisted.otlpTracesUrl),
    otlpMetricsUrl: Option.orElse(environment.otlpMetricsUrl, () => persisted.otlpMetricsUrl),
    otlpLogsUrl: Option.orElse(environment.otlpLogsUrl, () => persisted.otlpLogsUrl),
  } satisfies BackendObservabilitySettings;
});

interface SharedBootstrapInput {
  readonly bootstrapToken: string;
  readonly bootstrapSecret: string;
  readonly observabilitySettings: BackendObservabilitySettings;
  readonly hostScope: string;
}

// Matches the server's DESKTOP_HOST_SCOPE_ENV.
const DESKTOP_HOST_SCOPE_ENV = "T3CODE_DESKTOP_HOST_SCOPE";
const HOST_SCOPE_FILE_NAME = "desktop-host-scope";
const HOST_SCOPE_PATTERN = /^[0-9a-f]{32}$/;

/**
 * Not a credential: it only tells this install's Windows and WSL servers that
 * they run on one machine, so usage both read from one Windows directory counts
 * once. Persisted so summaries from earlier launches still match.
 */
export const readOrCreateHostScope = Effect.fn("desktop.readOrCreateHostScope")(function* (
  stateDirectory?: string,
) {
  const fileSystem = yield* FileSystem.FileSystem;
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const random = yield* SecureRandom.SecureRandom;
  const directory = stateDirectory ?? environment.stateDir;
  const scopePath = environment.path.join(directory, HOST_SCOPE_FILE_NAME);
  const existing = (yield* fileSystem
    .readFileString(scopePath)
    .pipe(Effect.orElseSucceed(() => ""))).trim();
  if (HOST_SCOPE_PATTERN.test(existing)) return existing;
  const scope = Hex.encode(yield* random.randomBytes(16).pipe(Effect.orDie));
  yield* fileSystem.makeDirectory(directory, { recursive: true }).pipe(
    Effect.andThen(fileSystem.writeFileString(scopePath, scope)),
    Effect.catch((cause) =>
      Effect.logWarning("Could not persist the desktop host scope.", { cause }),
    ),
  );
  return scope;
});

type WslPreflightRuntime =
  | {
      readonly kind: "executable";
      readonly entryPath: string;
    }
  | {
      readonly kind: "node-script";
      // Launch the exact Node path probed with node-pty, even if later PATH lookup would resolve another version.
      readonly nodePath: string;
      readonly linuxEntryPath: string;
    };

interface WslPreflightSuccess {
  readonly _tag: "Ready";
  readonly runningDistro: string;
  readonly windowsEntryPath: string;
  readonly runtime: WslPreflightRuntime;
  readonly resolvedPath: string;
  readonly runtimeId?: string;
}

interface WslPreflightFailure {
  readonly _tag: "Failed";
  readonly reason: string;
  readonly fatal: boolean;
  readonly retryLimit?: number;
}

const WSL_TRANSIENT_PREFLIGHT_RETRY_LIMIT = 12;
const WSL_RUNTIME_ARCHIVE_NAME = "wsl-runtime.tar.gz";
const WSL_RUNTIME_ARCHIVE_HASH_NAME = `${WSL_RUNTIME_ARCHIVE_NAME}.sha256`;
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/i;

const parseWslRuntimeArchiveHash = (value: string): string | null => {
  const trimmed = value.trim();
  return SHA256_HEX_PATTERN.test(trimmed) ? trimmed.toLowerCase() : null;
};

type FailedNodePtyResult = Extract<
  DesktopWslEnvironment.EnsureWslNodePtyResult,
  { readonly ok: false }
>;

const runWslPreflight = Effect.fn("desktop.backendConfiguration.wslPreflight")(function* (input: {
  readonly distro: string | null;
  readonly runtimeArchive: DesktopWslEnvironment.WslRuntimeArchive | null;
  readonly allowBuild: boolean;
}): Effect.fn.Return<
  WslPreflightSuccess | WslPreflightFailure,
  never,
  | DesktopEnvironment.DesktopEnvironment
  | DesktopWslEnvironment.DesktopWslEnvironment
  | DesktopWslServerTree.DesktopWslServerTree
  | FileSystem.FileSystem
> {
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const wslEnv = yield* DesktopWslEnvironment.DesktopWslEnvironment;
  const wslServerTree = yield* DesktopWslServerTree.DesktopWslServerTree;
  const fileSystem = yield* FileSystem.FileSystem;

  const wslAvailable = yield* wslEnv.isAvailable;
  if (!wslAvailable) {
    return {
      _tag: "Failed",
      reason: "WSL is not available on this system",
      fatal: false,
    } as const;
  }

  const distroProbe = yield* wslEnv.probeDistros.pipe(
    Effect.map((distros) => ({ _tag: "Success", distros }) as const),
    Effect.catch((error) => Effect.succeed({ _tag: "Failure", error } as const)),
  );
  if (distroProbe._tag === "Failure") {
    return {
      _tag: "Failed",
      reason: `Unable to list WSL distributions: ${distroProbe.error.message}`,
      fatal: false,
    } as const;
  }

  const installedDistros = distroProbe.distros;
  const runningDistro = input.distro
    ? (installedDistros.find(
        (installed) => installed.name.toLowerCase() === input.distro?.toLowerCase(),
      )?.name ?? null)
    : (installedDistros.find((installed) => installed.isDefault)?.name ?? null);
  if (runningDistro === null) {
    return {
      _tag: "Failed",
      reason: input.distro
        ? `WSL distro is not installed: ${input.distro}`
        : installedDistros.length === 0
          ? "WSL has no installed distributions"
          : "WSL has no default distribution",
      fatal: true,
    } as const;
  }

  const nodePtyOptions = {
    allowBuild: input.allowBuild,
    nodeEngineRange: serverPackageJson.engines.node,
  };
  const failedNodePty = (result: FailedNodePtyResult) =>
    ({
      _tag: "Failed",
      reason: `WSL node-pty unavailable: ${result.reason}`,
      fatal: result.fatal,
      ...(result.retryLimit === undefined ? {} : { retryLimit: result.retryLimit }),
    }) as const;

  const resolveMountedAppRoot = Effect.gen(function* () {
    const serverTree = yield* wslServerTree.ensure;
    if (!serverTree.ok) {
      return { ok: false, reason: serverTree.reason, fatal: serverTree.fatal } as const;
    }
    const serverDirectory = serverTree.serverDirectory ?? "apps/server";
    const serverEntry = serverTree.serverEntry ?? "dist/bin.mjs";
    const windowsEntryPath = environment.path.join(serverTree.root, serverDirectory, serverEntry);
    const entryExists = yield* fileSystem
      .exists(windowsEntryPath)
      .pipe(Effect.orElseSucceed(() => false));
    if (!entryExists) {
      return {
        ok: false,
        reason: `missing server entry at ${windowsEntryPath}`,
        fatal: true,
      } as const;
    }
    const mountedAppRoot = yield* wslEnv.windowsToWslPath(runningDistro, serverTree.root);
    return Option.isNone(mountedAppRoot)
      ? ({
          ok: false,
          reason: `wslpath conversion failed for ${serverTree.root}`,
          fatal: false,
        } as const)
      : ({
          ok: true,
          windowsEntryPath,
          serverDirectory,
          linuxEntryPath: `${mountedAppRoot.value}/${serverDirectory}/${serverEntry}`,
          linuxAppRoot: mountedAppRoot.value,
        } as const);
  });

  let stagedFailure: { readonly runtimeId?: string; readonly reason: string } | undefined;
  const failedStaged = (failure: { readonly reason: string }) =>
    ({
      _tag: "Failed",
      reason: `WSL runtime unavailable: ${failure.reason}`,
      fatal: true,
    }) as const;

  if (input.runtimeArchive !== null) {
    const runtime = yield* wslEnv.prepareRuntime(runningDistro, input.runtimeArchive);
    if (runtime.ok) {
      // Staged runtimes bundle Node and node-pty; provider PATH discovery must not require them.
      const stagedProbe = yield* wslEnv.probeRuntime(runningDistro);
      if (stagedProbe.ok) {
        yield* wslServerTree.cleanupLegacy;
        return {
          _tag: "Ready",
          runningDistro,
          windowsEntryPath: environment.backendEntryPath,
          runtime: { kind: "executable", entryPath: `${runtime.linuxAppRoot}/t3` },
          resolvedPath: stagedProbe.resolvedPath,
          runtimeId: input.runtimeArchive.runtimeId,
        } as const;
      }
      yield* Effect.logWarning(
        "The staged WSL runtime probe failed; retrying from the mounted server tree.",
        { reason: stagedProbe.reason },
      );
      stagedFailure = { runtimeId: input.runtimeArchive.runtimeId, reason: stagedProbe.reason };
    } else {
      stagedFailure = { reason: runtime.reason };
      yield* Effect.logWarning(
        "Could not stage the WSL runtime; launching from the mounted server tree instead.",
        { reason: runtime.reason },
      );
    }
  }

  const mounted = yield* resolveMountedAppRoot;
  if (!mounted.ok) {
    return stagedFailure && mounted.fatal
      ? failedStaged(stagedFailure)
      : ({ _tag: "Failed", reason: mounted.reason, fatal: mounted.fatal } as const);
  }

  const nodePtyResult = yield* wslEnv.ensureNodePty(runningDistro, mounted.linuxAppRoot, {
    ...nodePtyOptions,
    serverDirectory: mounted.serverDirectory,
  });
  if (!nodePtyResult.ok) {
    // Only substitute a staged failure for a fatal mounted failure; transient /mnt failures must remain retryable.
    return stagedFailure && nodePtyResult.fatal
      ? failedStaged(stagedFailure)
      : failedNodePty(nodePtyResult);
  }

  if (stagedFailure?.runtimeId) {
    yield* wslEnv.invalidateRuntime(runningDistro, stagedFailure.runtimeId);
  }

  return {
    _tag: "Ready",
    runningDistro,
    windowsEntryPath: mounted.windowsEntryPath,
    runtime: {
      kind: "node-script",
      nodePath: nodePtyResult.nodePath,
      linuxEntryPath: mounted.linuxEntryPath,
    },
    resolvedPath: nodePtyResult.resolvedPath,
  } as const;
});

// In WSL2 mirrored mode, eth0 matches a host IP; use loopback for the renderer.
const isLocalHostIpv4 = (ip: string): boolean => {
  const interfaces = NodeOS.networkInterfaces();
  for (const list of Object.values(interfaces)) {
    if (!list) continue;
    for (const entry of list) {
      const family = String(entry.family);
      if ((family === "IPv4" || family === "4") && entry.address === ip) return true;
    }
  }
  return false;
};

const buildObservabilityFragment = (observabilitySettings: BackendObservabilitySettings) => ({
  ...Option.match(observabilitySettings.otlpTracesUrl, {
    onNone: () => ({}),
    onSome: (otlpTracesUrl) => ({ otlpTracesUrl }),
  }),
  ...Option.match(observabilitySettings.otlpMetricsUrl, {
    onNone: () => ({}),
    onSome: (otlpMetricsUrl) => ({ otlpMetricsUrl }),
  }),
  ...Option.match(observabilitySettings.otlpLogsUrl, {
    onNone: () => ({}),
    onSome: (otlpLogsUrl) => ({ otlpLogsUrl }),
  }),
});

export const resolveWslStartConfig = Effect.fn("desktop.backendConfiguration.resolveWsl")(
  function* (
    input: SharedBootstrapInput & {
      readonly port: number;
      readonly distro: string | null;
      readonly requireControlReady?: boolean;
    },
  ): Effect.fn.Return<
    DesktopBackendStartConfig,
    never,
    | DesktopEnvironment.DesktopEnvironment
    | DesktopWslEnvironment.DesktopWslEnvironment
    | DesktopWslServerTree.DesktopWslServerTree
    | FileSystem.FileSystem
  > {
    const environment = yield* DesktopEnvironment.DesktopEnvironment;
    const wslEnvironment = yield* DesktopWslEnvironment.DesktopWslEnvironment;
    const fileSystem = yield* FileSystem.FileSystem;
    const env = yield* HostProcess.Environment;

    // wslhost loopback forwarding is unreliable on some Windows hosts; advertise the WSL IP instead.
    const wslBindHost = "0.0.0.0";

    const bootstrap = {
      mode: "desktop" as const,
      noBrowser: true,
      port: input.port,
      // Omit t3Home to keep the Linux backend from sharing the primary's Windows SQLite file.
      host: wslBindHost,
      ...(input.requireControlReady && { desktopTelemetryControlFd: 1, desktopAppUpdate: false }),
      desktopBootstrapToken: input.bootstrapToken,
      desktopBootstrapSecret: input.bootstrapSecret,
      // PortSchema rejects 0 even when Tailscale Serve is disabled.
      tailscaleServeEnabled: false,
      tailscaleServePort: 443,
      ...buildObservabilityFragment(input.observabilitySettings),
    };

    const archivePath = environment.path.join(environment.resourcesPath, WSL_RUNTIME_ARCHIVE_NAME);
    const archiveHashPath = environment.path.join(
      environment.resourcesPath,
      WSL_RUNTIME_ARCHIVE_HASH_NAME,
    );

    const hasArchive = environment.isPackaged
      ? yield* fileSystem.exists(archivePath).pipe(Effect.orElseSucceed(() => false))
      : false;
    const archiveHash = hasArchive
      ? yield* fileSystem.readFileString(archiveHashPath).pipe(
          Effect.map(parseWslRuntimeArchiveHash),
          Effect.orElseSucceed(() => null),
        )
      : null;
    if (hasArchive && archiveHash === null) {
      yield* Effect.logWarning(
        "Ignoring the WSL runtime archive because its SHA-256 identity is missing or invalid; launching from the mounted server tree instead.",
        { hashPath: archiveHashPath },
      );
    }

    const preflight = yield* runWslPreflight({
      distro: input.distro,
      runtimeArchive:
        archiveHash === null
          ? null
          : {
              windowsPath: archivePath,
              runtimeId: `sha256-${archiveHash}`,
              sha256: archiveHash,
            },
      allowBuild: !environment.isPackaged,
    });

    // Pin the preflight distro so a default change cannot race backend startup.
    const runningDistro = preflight._tag === "Ready" ? preflight.runningDistro : null;
    const distroForConfig = runningDistro ?? input.distro;

    const distroIp = yield* wslEnvironment.getDistroIp(distroForConfig);
    const usesSharedNetworkStack = Option.match(distroIp, {
      onNone: () => false,
      onSome: (ip) => isLocalHostIpv4(ip),
    });
    const rendererHost = usesSharedNetworkStack
      ? "127.0.0.1"
      : Option.getOrElse(distroIp, () => "127.0.0.1");
    const httpBaseUrl = new URL(`http://${rendererHost}:${input.port}`);

    const distroArgs = distroForConfig ? ["-d", distroForConfig] : [];
    const forwardedEnv: Record<string, string> = { [DESKTOP_HOST_SCOPE_ENV]: input.hostScope };
    const forwardedEnvNames: string[] = [DESKTOP_HOST_SCOPE_ENV];
    for (const name of WSL_FORWARDED_ENV_NAMES) {
      const value = env[name];
      if (value !== undefined && value.length > 0) {
        forwardedEnv[name] = value;
        forwardedEnvNames.push(name);
      }
    }

    const parentEnvWithoutT3Home: Record<string, string | undefined> = {};
    for (const [key, value] of Object.entries(env)) {
      if (["T3CODE_HOME", "T3CODE_RUNE_DEV_HOME"].includes(key.toUpperCase())) continue;
      parentEnvWithoutT3Home[key] = value;
    }
    const wslEnv = mergeWslEnv(parentEnvWithoutT3Home.WSLENV, forwardedEnvNames);

    const baseConfig = {
      executablePath: "wsl.exe",
      entryPath:
        preflight._tag === "Ready" ? preflight.windowsEntryPath : environment.backendEntryPath,
      cwd: environment.backendCwd,
      env: {
        ...parentEnvWithoutT3Home,
        ...backendChildEnvPatch(),
        ...forwardedEnv,
        ...(wslEnv !== undefined ? { WSLENV: wslEnv } : {}),
      },
      // env already excludes T3CODE_HOME; merging the host env would restore it.
      extendEnv: false,
      bootstrap,
      bootstrapDelivery: "stdin" as const,
      httpBaseUrl,
      captureOutput: true,
      ...(runningDistro !== null ? { runningDistro } : {}),
    };

    // Explicit --dev-url keeps WSL state in dev/ rather than userdata/.
    const devUrlArgs = Option.match(environment.devServerUrl, {
      onNone: () => [] as ReadonlyArray<string>,
      onSome: (url) => ["--dev-url", url.href],
    });

    if (preflight._tag === "Failed") {
      const retryLimit =
        preflight.retryLimit ?? (preflight.fatal ? undefined : WSL_TRANSIENT_PREFLIGHT_RETRY_LIMIT);
      return {
        ...baseConfig,
        args: [...distroArgs, "--", "node", "--version"],
        preflightFailure: Option.some({
          reason: preflight.reason,
          fatal: preflight.fatal,
          ...(retryLimit === undefined ? {} : { retryLimit }),
        }),
      } satisfies DesktopBackendStartConfig;
    }

    // wsl.exe --exec omits the login-shell PATH needed to find provider CLIs and npm.
    const runtime = preflight.runtime;
    const launchPath =
      runtime.kind === "executable"
        ? `${WSL_SERVER_SYSTEM_PATH}:${preflight.resolvedPath}`
        : `${nodeBinDirOf(runtime.nodePath)}:${WSL_SERVER_SYSTEM_PATH}:${preflight.resolvedPath}`;
    const command =
      runtime.kind === "executable"
        ? [runtime.entryPath]
        : [runtime.nodePath, runtime.linuxEntryPath];

    return {
      ...baseConfig,
      args: [
        ...distroArgs,
        // WSL otherwise inherits the Windows-mounted desktop working directory.
        "--cd",
        "~",
        "--exec",
        "env",
        `PATH=${launchPath}`,
        ...command,
        "--bootstrap-fd",
        "0",
        ...devUrlArgs,
      ],
      preflightFailure: Option.none(),
      ...(preflight.runtimeId === undefined ? {} : { wslRuntimeId: preflight.runtimeId }),
    } satisfies DesktopBackendStartConfig;
  },
);
