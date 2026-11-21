import type { OpenVisitDatabase } from "./visits.ts";
import * as PlatformNodeCrypto from "@effect/platform-node/NodeCrypto";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import { currentDesktopBootstrapToken } from "@t3tools/shared/desktopBootstrapToken";
import * as Net from "@t3tools/shared/Net";
import {
  SERVER_EXIT_CODE_STATE_DIR_OWNED,
  SERVER_EXIT_CODE_MANAGED_POLICY,
} from "@t3tools/contracts";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Exit from "effect/Exit";
import * as Cause from "effect/Cause";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import * as Hex from "effect/encoding/Hex";
import * as Fiber from "effect/Fiber";
import * as Logger from "effect/Logger";
import * as Option from "effect/Option";
import * as Stream from "effect/Stream";
import * as Schema from "effect/Schema";

import {
  backendChildEnvPatch,
  runBackendProcess,
  type DesktopBackendStartConfig,
} from "../backend/DesktopBackendProcess.ts";
import * as DesktopCliShim from "../app/DesktopCliShim.ts";
import { configuredBackendPort } from "../app/DesktopConfig.ts";
import * as DesktopManagedPolicy from "../app/DesktopManagedPolicy.ts";
import { resolveDesktopBackendPort } from "../backend/DesktopBackendPort.ts";
import * as DesktopNetworkInterfaces from "../backend/DesktopNetworkInterfaces.ts";
import { readDesktopBackendExposure } from "../backend/DesktopServerExposure.ts";
import {
  readDesktopSettingsProfile,
  type DesktopSettings,
} from "../settings/DesktopAppSettings.ts";
import { pairWithTarget } from "./projects.ts";

class LocalServerStartupError extends Schema.TaggedError<LocalServerStartupError>()(
  "LocalServerStartupError",
  { message: Schema.String },
) {}

export const isLocalServerStartupError = Schema.is(LocalServerStartupError);

interface LocalServerPaths {
  readonly packaged: boolean;
  readonly version: string;
  readonly entryPath: string;
  readonly cwd: string;
  readonly t3Home: string;
  readonly previousSettingsHome?: string;
  readonly dev?: { readonly port: number; readonly webUrl?: string | undefined };
}

export const makeNativeServerConfig = Effect.fn("nativeDesktop.makeServerConfig")(function* (
  paths: LocalServerPaths,
  settings: DesktopSettings,
  desktopBootstrapToken: string,
  secret: string,
  cliPath?: string,
) {
  const { entryPath, cwd, t3Home } = paths;
  const net = yield* Net.NetService;
  const port = paths.packaged
    ? (yield* resolveDesktopBackendPort(yield* configuredBackendPort)).port
    : (paths.dev?.port ?? (yield* net.reserveLoopbackPort()));
  const exposure = paths.packaged
    ? yield* readDesktopBackendExposure(settings, port)
    : {
        httpBaseUrl: new URL(`http://127.0.0.1:${port}`),
        bindHost: "127.0.0.1",
        tailscaleServeEnabled: false,
        tailscaleServePort: 443,
      };

  return {
    executablePath: process.execPath,
    entryPath,
    cwd,
    args: [entryPath, "--bootstrap-fd", "3", "--base-dir", t3Home],
    env: {
      ...backendChildEnvPatch(),
      ...(paths.packaged && { T3CODE_CLI_PATH: cliPath }),
      T3CODE_RUNE_DEV_HOME: undefined,
      T3CODE_HOME: t3Home,
      VITE_DEV_SERVER_URL: paths.dev?.webUrl,
    },
    extendEnv: true,
    captureOutput: true,
    preflightFailure: Option.none(),
    bootstrapDelivery: "fd3",
    httpBaseUrl: exposure.httpBaseUrl,
    bootstrap: {
      mode: "desktop",
      noBrowser: true,
      port,
      host: exposure.bindHost,
      t3Home,
      desktopBootstrapToken,
      desktopBootstrapSecret: secret,
      desktopTelemetryControlFd: 4,
      desktopAppUpdate: false,
      tailscaleServeEnabled: exposure.tailscaleServeEnabled,
      tailscaleServePort: exposure.tailscaleServePort,
    },
  } satisfies DesktopBackendStartConfig;
});

export function startLocalServer(paths: LocalServerPaths) {
  const { t3Home } = paths;
  const lifetime = new AbortController();
  const access = new AbortController();
  const ready = Promise.withResolvers<{ url: string; sameFilesystem: boolean }>();
  const secret = Hex.encode(globalThis.crypto.getRandomValues(new Uint8Array(32)));
  const currentToken = () =>
    Clock.currentTimeMillis.pipe(
      Effect.flatMap((now) => currentDesktopBootstrapToken(secret, now)),
      Effect.provide(PlatformNodeCrypto.layerMac),
    );
  const run = Effect.gen(function* () {
    const startupTimeout = yield* Effect.sleep("3 minutes").pipe(
      Effect.andThen(
        Effect.sync(() =>
          lifetime.abort(
            new LocalServerStartupError({ message: "The local server timed out during startup." }),
          ),
        ),
      ),
      Effect.forkScoped,
    );
    const path = yield* Path.Path;
    const { settings, path: settingsPath } = yield* readDesktopSettingsProfile(
      yield* FileSystem.FileSystem,
      path.join(t3Home, "userdata", "desktop-settings.json"),
      paths.version,
      paths.previousSettingsHome &&
        path.join(paths.previousSettingsHome, "userdata", "desktop-settings.json"),
    );
    const cliPath = paths.packaged
      ? Option.getOrUndefined(yield* DesktopCliShim.installBundledNode(t3Home, paths.entryPath))
      : undefined;
    if (!settings.localEnvironmentEnabled)
      return yield* new LocalServerStartupError({
        message: "The local environment is disabled. Paste a pairing link to connect to a server.",
      });
    const local = yield* makeNativeServerConfig(
      paths,
      settings,
      yield* currentToken(),
      secret,
      cliPath,
    );
    const { config, afterReady } =
      (yield* HostProcess.Platform) === "win32"
        ? yield* Effect.promise(() => import("./windowsHost.ts")).pipe(
            Effect.flatMap(({ prepareWindowsHost }) =>
              prepareWindowsHost(local, paths, settings, path.dirname(settingsPath)),
            ),
          )
        : { config: local, afterReady: Effect.void };
    return yield* runBackendProcess({
      ...config,
      desktopTelemetryStream: Stream.empty,
      requireControlReady: true,
      onOutput: (stream, chunk) =>
        stream === "stderr"
          ? Effect.callback<void, Error>((resume) => {
              process.stderr.write(chunk, (error) =>
                resume(error ? Effect.fail(error) : Effect.void),
              );
            })
          : Effect.void,
      onExitObserved: () => Effect.sync(() => access.abort()),
      onReady: () =>
        Fiber.interrupt(startupTimeout).pipe(
          Effect.andThen(
            Effect.sync(() =>
              ready.resolve({
                url: config.httpBaseUrl.href,
                sameFilesystem: !("runningDistro" in config && config.runningDistro),
              }),
            ),
          ),
          Effect.andThen(afterReady),
        ),
    });
  });
  const stopped = Effect.runPromiseExit(
    run.pipe(
      Effect.scoped,
      Effect.provide([Net.layer, DesktopManagedPolicy.layer, DesktopNetworkInterfaces.layer]),
      Effect.provide([
        NodeServices.layer,
        PlatformNodeCrypto.layerSecureRandom,
        PlatformNodeCrypto.layerDigest,
        layerRemoteHttpClient(globalThis.fetch),
      ]),
      Effect.provideService(Logger.LogToStderr, true),
    ),
    { signal: lifetime.signal },
  ).then((exit) => {
    access.abort();
    let message = "The local server stopped.";
    const timeout = isLocalServerStartupError(lifetime.signal.reason);
    if (timeout || !lifetime.signal.aborted) {
      if (timeout) {
        message = lifetime.signal.reason.message;
      } else if (Exit.isSuccess(exit)) {
        const code = Option.getOrUndefined(exit.value.code);
        if (code === SERVER_EXIT_CODE_STATE_DIR_OWNED)
          message = "Another T3 Code server owns this data directory.";
        else if (code === SERVER_EXIT_CODE_MANAGED_POLICY)
          message = "Managed policy prevented the local server from starting.";
        else message = `Local server exited (${exit.value.reason}).`;
      } else {
        const error = Cause.squash(exit.cause);
        message =
          error instanceof Error ? error.message : "The local server process could not run.";
      }
      process.stderr.write(
        `[Rune desktop] ${message} See ${t3Home}/userdata/logs for server diagnostics.\n`,
      );
    }
    ready.reject(new LocalServerStartupError({ message }));
  });
  return {
    async connect(workerSignal: AbortSignal, openVisits?: OpenVisitDatabase) {
      const signal = AbortSignal.any([workerSignal, lifetime.signal, access.signal]);
      const { url, sameFilesystem } = await ready.promise;
      const credential = await Effect.runPromise(currentToken(), { signal });
      const session = await pairWithTarget(
        { httpBaseUrl: url, credential },
        signal,
        undefined,
        openVisits,
      );
      return { ...session, sameFilesystem };
    },
    stopped,
    stop() {
      lifetime.abort();
      return stopped;
    },
  };
}
