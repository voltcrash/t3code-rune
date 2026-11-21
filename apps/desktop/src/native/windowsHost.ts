import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Digest from "effect/crypto/Digest";
import * as ConfigProvider from "effect/ConfigProvider";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as DesktopEnvironment from "../app/DesktopEnvironment.ts";
import * as DesktopManagedPolicy from "../app/DesktopManagedPolicy.ts";
import {
  readBackendObservabilitySettings,
  readOrCreateHostScope,
  resolveWslStartConfig,
} from "../backend/DesktopBackendConfiguration.ts";
import {
  backendRestartDelay,
  type DesktopBackendStartConfig,
} from "../backend/DesktopBackendProcess.ts";
import type { DesktopSettings } from "../settings/DesktopAppSettings.ts";
import * as Wsl from "../wsl/DesktopWslEnvironment.ts";
import { DesktopWslServerTree } from "../wsl/DesktopWslServerTree.ts";

interface LocalBootstrap {
  readonly port: number;
  readonly desktopBootstrapToken: string;
  readonly desktopBootstrapSecret: string;
}

export const resolveNativeWslHost = Effect.fn("nativeDesktop.resolveWslHost")(function* (
  bootstrap: LocalBootstrap,
  distro: string | null,
  hostScope: string,
) {
  const wsl = yield* Wsl.DesktopWslEnvironment;
  if (!(yield* wsl.isAvailable)) return undefined;
  const resolveConfig = resolveWslStartConfig({
    port: bootstrap.port,
    distro,
    hostScope,
    bootstrapToken: bootstrap.desktopBootstrapToken,
    bootstrapSecret: bootstrap.desktopBootstrapSecret,
    observabilitySettings: yield* readBackendObservabilitySettings,
    requireControlReady: true,
  });
  let config = yield* resolveConfig;
  for (let attempt = 1; Option.isSome(config.preflightFailure); attempt++) {
    const { fatal, retryLimit } = config.preflightFailure.value;
    if (fatal || (retryLimit !== undefined && attempt >= retryLimit)) break;
    yield* Effect.sleep(backendRestartDelay(attempt - 1));
    config = yield* resolveConfig;
  }
  if (Option.isSome(config.preflightFailure)) {
    yield* Effect.logWarning("WSL startup failed. Using the Windows environment.", {
      reason: config.preflightFailure.value.reason,
    });
    return undefined;
  }
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  if (!environment.isPackaged) {
    const digest = yield* Digest.Digest;
    const checkout = yield* digest.digestHex(
      Digest.sha256,
      new TextEncoder().encode(environment.path.resolve(environment.baseDir)),
    );
    config = {
      ...config,
      args: [...config.args, "--base-dir", `~/.t3/rune-dev/${checkout.slice(0, 32)}`],
    };
  }
  return {
    config,
    afterReady:
      config.wslRuntimeId && config.runningDistro
        ? wsl.pruneRuntimes(config.runningDistro, config.wslRuntimeId)
        : Effect.void,
  };
});

export const prepareWindowsHost = Effect.fn("nativeDesktop.prepareWindowsHost")(function* (
  local: DesktopBackendStartConfig & { readonly bootstrap: LocalBootstrap },
  paths: { readonly t3Home: string; readonly packaged: boolean; readonly version: string },
  settings: DesktopSettings,
) {
  const path = yield* Path.Path;
  const root = path.resolve(local.cwd, paths.packaged ? ".." : "../..");
  const environment = DesktopEnvironment.layer({
    platform: "win32",
    dirname: path.join(root, "apps/desktop/dist"),
    appPath: root,
    isPackaged: paths.packaged,
    resourcesPath: root,
    serverDirectory: local.cwd,
    homeDirectory: yield* HostProcess.HomeDirectory,
    processArch: yield* HostProcess.Architecture,
    appVersion: paths.version,
    runningUnderArm64Translation: false,
  }).pipe(
    Layer.provide(
      ConfigProvider.layer(
        ConfigProvider.fromEnvRecord({
          ...(yield* HostProcess.Environment),
          T3CODE_HOME: paths.t3Home,
          VITE_DEV_SERVER_URL: local.env.VITE_DEV_SERVER_URL,
        }),
      ),
    ),
  );
  return yield* Effect.gen(function* () {
    const hostScope = yield* readOrCreateHostScope;
    const windows = {
      config: { ...local, env: { ...local.env, T3CODE_DESKTOP_HOST_SCOPE: hostScope } },
      afterReady: Effect.void,
    };
    if (!settings.wslBackendEnabled || !settings.wslOnly) return windows;
    const wsl = yield* resolveNativeWslHost(local.bootstrap, settings.wslDistro, hostScope).pipe(
      Effect.provide(Wsl.layer.pipe(Layer.provide(DesktopManagedPolicy.layer))),
      Effect.provideService(DesktopWslServerTree, {
        ensure: Effect.succeed({
          ok: true as const,
          root,
          serverDirectory: path.relative(root, local.cwd).split(path.sep).join("/"),
          serverEntry: path.relative(local.cwd, local.entryPath).split(path.sep).join("/"),
        }),
        cleanupLegacy: Effect.void,
      }),
    );
    return wsl ?? windows;
  }).pipe(Effect.provide(environment));
});
