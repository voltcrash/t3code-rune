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

import { backendChildEnvPatch, runBackendProcess } from "../backend/DesktopBackendProcess.ts";
import { pairWithTarget } from "./projects.ts";

class LocalServerStartupError extends Schema.TaggedError<LocalServerStartupError>()(
  "LocalServerStartupError",
  { message: Schema.String },
) {}

export const isLocalServerStartupError = Schema.is(LocalServerStartupError);

export function startLocalServer(paths: {
  readonly entryPath: string;
  readonly cwd: string;
  readonly t3Home: string;
  readonly dev?: { readonly port: number; readonly webUrl?: string | undefined };
}) {
  const { entryPath, cwd, t3Home } = paths;
  const lifetime = new AbortController();
  const access = new AbortController();
  const ready = Promise.withResolvers<string>();
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
    const port = paths.dev?.port ?? (yield* Net.make().reserveLoopbackPort());
    const httpBaseUrl = new URL(`http://127.0.0.1:${port}`);
    const desktopBootstrapToken = yield* currentToken();
    return yield* runBackendProcess({
      executablePath: process.execPath,
      entryPath,
      cwd,
      args: [entryPath, "--bootstrap-fd", "3", "--base-dir", t3Home],
      env: {
        ...backendChildEnvPatch(),
        T3CODE_RUNE_DEV_HOME: undefined,
        T3CODE_HOME: t3Home,
        VITE_DEV_SERVER_URL: paths.dev?.webUrl,
      },
      extendEnv: true,
      captureOutput: true,
      preflightFailure: Option.none(),
      bootstrapDelivery: "fd3",
      httpBaseUrl,
      bootstrap: {
        mode: "desktop",
        noBrowser: true,
        port,
        host: "127.0.0.1",
        t3Home,
        desktopBootstrapToken,
        desktopBootstrapSecret: secret,
        desktopTelemetryControlFd: 4,
        desktopAppUpdate: false,
        tailscaleServeEnabled: false,
        tailscaleServePort: 443,
      },
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
          Effect.andThen(Effect.sync(() => ready.resolve(httpBaseUrl.href))),
        ),
    });
  });
  const stopped = Effect.runPromiseExit(
    run.pipe(
      Effect.scoped,
      Effect.provide([NodeServices.layer, layerRemoteHttpClient(globalThis.fetch)]),
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
      const url = await ready.promise;
      const credential = await Effect.runPromise(currentToken(), { signal });
      return pairWithTarget({ httpBaseUrl: url, credential }, signal, undefined, openVisits);
    },
    stopped,
    stop() {
      lifetime.abort();
      return stopped;
    },
  };
}
