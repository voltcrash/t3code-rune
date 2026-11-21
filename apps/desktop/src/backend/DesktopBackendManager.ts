import * as Brand from "effect/Brand";
import * as Cause from "effect/Cause";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Fiber from "effect/Fiber";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as PlatformError from "effect/PlatformError";
import * as Ref from "effect/Ref";
import * as SubscriptionRef from "effect/SubscriptionRef";
import * as Semaphore from "effect/Semaphore";
import * as Scope from "effect/Scope";
import * as Stream from "effect/Stream";
import { HttpClient } from "effect/http";
import { ChildProcessSpawner } from "effect/process";

import {
  SERVER_EXIT_CODE_MANAGED_POLICY,
  SERVER_EXIT_CODE_STATE_DIR_OWNED,
  PRIMARY_LOCAL_ENVIRONMENT_ID,
} from "@t3tools/contracts";
import { isRunningDatabaseMigrations } from "@t3tools/shared/databaseMigrationLog";

import * as DesktopObservability from "../app/DesktopObservability.ts";
import * as DesktopTelemetryPublisher from "../telemetry/DesktopTelemetryPublisher.ts";
import * as DesktopBrowserHost from "../preview/DesktopBrowserHost.ts";
import * as DesktopWslEnvironment from "../wsl/DesktopWslEnvironment.ts";

import {
  backendRestartDelay,
  runBackendProcess,
  type DesktopBackendStartConfig,
  type PreflightFailure,
} from "./DesktopBackendProcess.ts";

export {
  BACKEND_QUIT_STOP_TIMEOUT,
  runBackendProcess,
  type DesktopBackendStartConfig,
  type PreflightFailure,
} from "./DesktopBackendProcess.ts";

const MAX_PREFLIGHT_FAILURE_ATTEMPTS = 5;
// Three restarts without readiness is about 2 s of backoff: past a blip, early enough to help.
const STARTUP_FAILURE_RESTART_ATTEMPTS = 3;
const STARTUP_FAILURE_OUTPUT_CHARS = 4_000;
const { logWarning: logBackendProcessWarning } =
  DesktopObservability.makeComponentLogger("desktop-backend-process");

export interface DesktopBackendSnapshot {
  readonly desiredRunning: boolean;
  readonly ready: boolean;
  readonly activePid: Option.Option<number>;
  readonly restartAttempt: number;
  readonly restartScheduled: boolean;
}

export type BackendInstanceId = string & Brand.Brand<"BackendInstanceId">;
export const BackendInstanceId = Brand.nominal<BackendInstanceId>();

export const PRIMARY_INSTANCE_ID: BackendInstanceId = BackendInstanceId(
  PRIMARY_LOCAL_ENVIRONMENT_ID,
);

export interface BackendStartupFailure {
  readonly reason: string;
  /** The server refused to start under this machine's managed policy. */
  readonly managedPolicy: boolean;
  /** Tail of the last run's stdout and stderr. */
  readonly output: string;
}

export interface DesktopBackendInstance {
  readonly id: BackendInstanceId;
  readonly label: Effect.Effect<string>;
  readonly start: Effect.Effect<void>;
  /** Timeout controls the warning; stop still waits for process teardown and exit. */
  readonly stop: (options?: { readonly timeout?: Duration.Duration }) => Effect.Effect<void>;
  readonly currentConfig: Effect.Effect<Option.Option<DesktopBackendStartConfig>>;
  readonly snapshot: Effect.Effect<DesktopBackendSnapshot>;
  readonly waitForReady: (timeout: Duration.Duration) => Effect.Effect<boolean>;
}

export interface BackendInstanceSpec {
  readonly id: BackendInstanceId;
  readonly label: Effect.Effect<string>;
  readonly configResolve: Effect.Effect<DesktopBackendStartConfig, PlatformError.PlatformError>;
  /** Receives the ready run's URL and pid; reading current config could race a subsequent restart. */
  readonly onReady?: (httpBaseUrl: URL, pid: number) => Effect.Effect<void>;
  readonly onShutdown?: () => Effect.Effect<void>;
  readonly onPreflightFailed?: (failure: PreflightFailure) => Effect.Effect<boolean>;
  readonly onStateDirOwned?: () => Effect.Effect<void>;
  /** Fires on each repeated crash or readiness timeout until the backend becomes ready. */
  readonly onStartupFailure?: (failure: BackendStartupFailure) => Effect.Effect<void>;
}

interface ActiveBackendRun {
  readonly id: number;
  readonly scope: Scope.Closeable;
  readonly fiber: Option.Option<Fiber.Fiber<void, never>>;
  readonly pid: Option.Option<number>;
  readonly exitObserved: boolean;
  readonly stopRequested: boolean;
}

interface BackendManagerState {
  readonly desiredRunning: boolean;
  readonly ready: boolean;
  readonly config: Option.Option<DesktopBackendStartConfig>;
  readonly active: Option.Option<ActiveBackendRun>;
  readonly restartAttempt: number;
  readonly preflightFailureAttempt: number;
  readonly restartFiber: Option.Option<Fiber.Fiber<void, never>>;
  readonly nextRunId: number;
}

const initialState: BackendManagerState = {
  desiredRunning: false,
  ready: false,
  config: Option.none(),
  active: Option.none(),
  restartAttempt: 0,
  preflightFailureAttempt: 0,
  restartFiber: Option.none(),
  nextRunId: 1,
};

const activePid = (active: Option.Option<ActiveBackendRun>): Option.Option<number> =>
  Option.flatMap(active, (run) => run.pid);

const withActiveRun =
  (runId: number, f: (run: ActiveBackendRun) => ActiveBackendRun) =>
  (state: BackendManagerState): BackendManagerState => ({
    ...state,
    active: Option.map(state.active, (run) => (run.id === runId ? f(run) : run)),
  });

const closeRun = (
  run: ActiveBackendRun,
  parentScope: Scope.Scope,
  options?: { readonly timeout?: Duration.Duration },
) => {
  const waitForFiber = Option.match(run.fiber, {
    onNone: () => Effect.void,
    onSome: (fiber) => Fiber.await(fiber).pipe(Effect.asVoid),
  });
  const close = Scope.close(run.scope, Exit.void).pipe(
    Effect.withSpan("desktop.backendInstance.closeScope"),
    Effect.andThen(waitForFiber.pipe(Effect.withSpan("desktop.backendInstance.awaitRun"))),
  );
  const timeout = options?.timeout;

  if (!timeout) {
    return close;
  }

  return Effect.forkIn(close, parentScope, { startImmediately: true }).pipe(
    Effect.flatMap((closeFiber) =>
      Fiber.await(closeFiber).pipe(
        Effect.timeoutOption(timeout),
        Effect.tap((completed) =>
          Option.isNone(completed)
            ? logBackendProcessWarning("backend shutdown exceeded timeout; waiting for exit", {
                timeoutMs: Duration.toMillis(timeout),
              })
            : Effect.void,
        ),
        // Parent-scope interruption still finishes release; do not cancel sibling stops.
        Effect.andThen(Fiber.await(closeFiber)),
        Effect.asVoid,
      ),
    ),
  );
};

export const makeBackendInstance = Effect.fn("makeBackendInstance")(function* (
  spec: BackendInstanceSpec,
): Effect.fn.Return<
  DesktopBackendInstance,
  never,
  | FileSystem.FileSystem
  | ChildProcessSpawner.ChildProcessSpawner
  | HttpClient.HttpClient
  | DesktopObservability.DesktopBackendOutputLogFactory
  | DesktopTelemetryPublisher.DesktopTelemetryPublisher
  | DesktopBrowserHost.DesktopBrowserHost
  | DesktopWslEnvironment.DesktopWslEnvironment
  | Scope.Scope
> {
  const parentScope = yield* Scope.Scope;
  const fileSystem = yield* FileSystem.FileSystem;
  const backendOutputLogFactory = yield* DesktopObservability.DesktopBackendOutputLogFactory;
  const backendOutputLog = yield* backendOutputLogFactory.forInstance(spec.id);
  const desktopTelemetryPublisher = yield* DesktopTelemetryPublisher.DesktopTelemetryPublisher;
  const desktopBrowserHost = yield* DesktopBrowserHost.DesktopBrowserHost;
  const wslEnvironment = yield* DesktopWslEnvironment.DesktopWslEnvironment;
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const httpClient = yield* HttpClient.HttpClient;
  const state = yield* SubscriptionRef.make(initialState);
  const mutex = yield* Semaphore.make(1);
  const outputTail = yield* Ref.make("");
  // One streaming decoder per stream keeps UTF-8 characters split across chunks intact.
  const makeOutputDecoders = () => ({ stdout: new TextDecoder(), stderr: new TextDecoder() });
  let outputDecoders = makeOutputDecoders();
  const reportStartupFailure = (reason: string, managedPolicy = false) =>
    Ref.get(outputTail).pipe(
      Effect.flatMap(
        (output) => spec.onStartupFailure?.({ reason, managedPolicy, output }) ?? Effect.void,
      ),
    );

  const { logWarning: logInstanceWarning, logError: logInstanceError } =
    DesktopObservability.makeComponentLogger(`desktop-backend-instance:${spec.id}`);

  const updateActiveRun = (runId: number, f: (run: ActiveBackendRun) => ActiveBackendRun) =>
    SubscriptionRef.update(state, withActiveRun(runId, f));

  const snapshot = SubscriptionRef.get(state).pipe(
    Effect.map((current): DesktopBackendSnapshot => ({
      desiredRunning: current.desiredRunning,
      ready: current.ready,
      activePid: activePid(current.active),
      restartAttempt: current.restartAttempt,
      restartScheduled: Option.isSome(current.restartFiber),
    })),
  );
  const currentConfig = SubscriptionRef.get(state).pipe(Effect.map((current) => current.config));

  const cancelRestart = Effect.gen(function* () {
    const restartFiber = yield* SubscriptionRef.modify(state, (current) => [
      current.restartFiber,
      {
        ...current,
        restartFiber: Option.none(),
      },
    ]);

    yield* Option.match(restartFiber, {
      onNone: () => Effect.void,
      onSome: (fiber) => Fiber.interrupt(fiber).pipe(Effect.asVoid),
    });
  });

  const start: Effect.Effect<void> = Effect.suspend(() =>
    mutex.withPermits(1)(
      Effect.gen(function* () {
        const current = yield* SubscriptionRef.get(state);
        if (Option.isSome(current.active)) {
          if (!current.desiredRunning) {
            yield* SubscriptionRef.update(state, (latest) => ({
              ...latest,
              desiredRunning: true,
            }));
          }
          return;
        }

        if (current.ready) {
          yield* spec.onShutdown?.() ?? Effect.void;
          yield* SubscriptionRef.update(state, (latest) =>
            latest.ready ? { ...latest, ready: false } : latest,
          );
        }
        const config = yield* spec.configResolve.pipe(
          Effect.tapError((error) =>
            logInstanceError("failed to generate desktop backend configuration", {
              cause: error.message,
            }),
          ),
          Effect.option,
        );
        if (Option.isNone(config)) {
          if (current.desiredRunning) {
            yield* scheduleRestart("failed to generate desktop backend configuration");
          }
          return;
        }
        const entryExists = yield* fileSystem
          .exists(config.value.entryPath)
          .pipe(Effect.orElseSucceed(() => false));

        const resetFatalPreflightCounter =
          !current.desiredRunning && current.preflightFailureAttempt > 0;
        yield* cancelRestart;
        yield* SubscriptionRef.update(state, (latest) => ({
          ...latest,
          desiredRunning: true,
          ready: false,
          config: Option.some(config.value),
          preflightFailureAttempt: resetFatalPreflightCounter ? 0 : latest.preflightFailureAttempt,
        }));

        const preflightFailure = config.value.preflightFailure;
        if (Option.isSome(preflightFailure)) {
          const { reason, fatal, retryLimit } = preflightFailure.value;
          if (!fatal && retryLimit === undefined) {
            yield* SubscriptionRef.update(state, (latest) =>
              latest.preflightFailureAttempt === 0
                ? latest
                : { ...latest, preflightFailureAttempt: 0 },
            );
            yield* scheduleRestart(reason);
            return;
          }
          const attemptLimit = retryLimit ?? MAX_PREFLIGHT_FAILURE_ATTEMPTS;
          const attempt = yield* SubscriptionRef.modify(state, (latest) => {
            const next = latest.preflightFailureAttempt + 1;
            return [next, { ...latest, preflightFailureAttempt: next }] as const;
          });
          if (attempt > attemptLimit) {
            yield* logInstanceError("backend preflight still failing after fallback; stopping", {
              reason,
              attempt,
            });
            yield* SubscriptionRef.update(state, (latest) => ({
              ...latest,
              desiredRunning: false,
              ready: false,
            }));
            return;
          }
          if (attempt === attemptLimit) {
            yield* logInstanceError(
              "backend preflight failed repeatedly; surfacing and falling back",
              { reason, attempt },
            );
            const shouldRestart = yield* (
              spec.onPreflightFailed?.(preflightFailure.value) ?? Effect.succeed(false)
            );
            if (shouldRestart) {
              yield* scheduleRestart(reason);
            } else {
              yield* SubscriptionRef.update(state, (latest) => ({
                ...latest,
                desiredRunning: false,
                ready: false,
              }));
            }
            return;
          }
          yield* scheduleRestart(reason);
          return;
        }
        yield* SubscriptionRef.update(state, (latest) =>
          latest.preflightFailureAttempt === 0 ? latest : { ...latest, preflightFailureAttempt: 0 },
        );

        if (!entryExists) {
          yield* scheduleRestart(`missing server entry at ${config.value.entryPath}`);
          return;
        }

        const runScope = yield* Scope.make("sequential");
        const runId = yield* SubscriptionRef.modify(state, (latest) => [
          latest.nextRunId,
          {
            ...latest,
            active: Option.some({
              id: latest.nextRunId,
              scope: runScope,
              fiber: Option.none(),
              pid: Option.none(),
              exitObserved: false,
              stopRequested: false,
            } satisfies ActiveBackendRun),
            nextRunId: latest.nextRunId + 1,
          },
        ]);

        const finalizeRun = Effect.fn("desktop.backendInstance.finalizeRun")(function* (
          reason: string,
          exitCode?: number,
        ) {
          const stateDirOwned = exitCode === SERVER_EXIT_CODE_STATE_DIR_OWNED;
          yield* mutex.withPermits(1)(
            Effect.gen(function* () {
              const { isCurrentRun, nextState, pid, exitObserved, stopRequested, wasReady } =
                yield* SubscriptionRef.modify(
                  state,
                  (
                    latest,
                  ): readonly [
                    {
                      readonly isCurrentRun: boolean;
                      readonly nextState: BackendManagerState;
                      readonly pid: Option.Option<number>;
                      readonly exitObserved: boolean;
                      readonly stopRequested: boolean;
                      readonly wasReady: boolean;
                    },
                    BackendManagerState,
                  ] => {
                    const currentRun = Option.getOrUndefined(latest.active);
                    if (currentRun?.id !== runId) {
                      return [
                        {
                          isCurrentRun: false,
                          nextState: latest,
                          pid: Option.none<number>(),
                          exitObserved: false,
                          stopRequested: false,
                          wasReady: false,
                        },
                        latest,
                      ] as const;
                    }

                    const next = {
                      ...latest,
                      active: Option.none<ActiveBackendRun>(),
                      ready: false,
                    };
                    return [
                      {
                        isCurrentRun: true,
                        nextState: next,
                        pid: currentRun.pid,
                        exitObserved: currentRun.exitObserved,
                        stopRequested: currentRun.stopRequested,
                        wasReady: latest.ready,
                      },
                      next,
                    ] as const;
                  },
                );

              if (isCurrentRun) {
                yield* desktopTelemetryPublisher.removeControlSource(spec.id);
                if (Option.isSome(pid)) {
                  if (exitObserved && !stopRequested) {
                    yield* backendOutputLog.persistFailure({
                      details: `pid=${pid.value} ${reason}`,
                    });
                  } else {
                    yield* backendOutputLog.discardSession;
                  }
                }
                if (wasReady) {
                  yield* spec.onShutdown?.() ?? Effect.void;
                }
              }

              if (isCurrentRun && nextState.desiredRunning) {
                if (stateDirOwned && !stopRequested) {
                  yield* SubscriptionRef.update(state, (latest) => ({
                    ...latest,
                    desiredRunning: false,
                  }));
                  yield* (spec.onStateDirOwned?.() ?? Effect.void).pipe(Effect.ignore);
                  return;
                }
                yield* scheduleRestart(reason, exitCode);
              }
            }),
          );
        });

        const program = runBackendProcess({
          ...config.value,
          desktopTelemetryStream: desktopTelemetryPublisher.encoded,
          desktopBrowserStream: desktopBrowserHost.events,
          onDesktopBrowserCommand: desktopBrowserHost.handleCommandLine,
          onDesktopTelemetryControl: (message) =>
            desktopTelemetryPublisher.handleControlForSource(spec.id, message),
          onStarted: Effect.fn("desktop.backendInstance.onStarted")(function* (pid) {
            yield* updateActiveRun(runId, (run) => ({
              ...run,
              pid: Option.some(pid),
            }));
            yield* Ref.set(outputTail, "");
            outputDecoders = makeOutputDecoders();
            yield* backendOutputLog.beginSession({
              details: `pid=${pid} port=${config.value.bootstrap.port} cwd=${config.value.cwd}`,
            });
          }),
          onExitObserved: () =>
            updateActiveRun(runId, (run) => ({
              ...run,
              exitObserved: true,
            })),
          onReady: Effect.fn("desktop.backendInstance.onReady")(function* (pid) {
            const isCurrentRun = yield* SubscriptionRef.modify(state, (latest) => {
              const activeRun = Option.getOrUndefined(latest.active);
              if (activeRun?.id !== runId) {
                return [false, latest] as const;
              }

              return [
                true,
                {
                  ...latest,
                  restartAttempt: 0,
                  ready: true,
                },
              ] as const;
            });
            if (!isCurrentRun) {
              return;
            }

            yield* spec.onReady?.(config.value.httpBaseUrl, pid) ?? Effect.void;
            if (
              config.value.runningDistro !== undefined &&
              config.value.wslRuntimeId !== undefined
            ) {
              yield* wslEnvironment.pruneRuntimes(
                config.value.runningDistro,
                config.value.wslRuntimeId,
              );
            }
          }),
          onReadinessFailure: Effect.fn("desktop.backendInstance.onReadinessFailure")(
            function* (error) {
              if (isRunningDatabaseMigrations(yield* Ref.get(outputTail))) {
                yield* logInstanceWarning("backend is still migrating its database; waiting", {
                  error: error.message,
                });
                return;
              }
              yield* logInstanceWarning("backend readiness check failed during bootstrap", {
                error: error.message,
              });
              yield* backendOutputLog.persistFailureSnapshot({
                details: error.message,
              });
              yield* reportStartupFailure("The server started but did not become ready in time.");
            },
          ),
          onOutput: (streamName, chunk) =>
            Ref.update(outputTail, (tail) =>
              // A cut through a surrogate pair would make encodeURIComponent throw when rendering.
              (tail + outputDecoders[streamName].decode(chunk, { stream: true }))
                .slice(-STARTUP_FAILURE_OUTPUT_CHARS)
                .toWellFormed(),
            ).pipe(Effect.andThen(backendOutputLog.writeOutputChunk(streamName, chunk))),
        }).pipe(
          Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, spawner),
          Effect.provideService(HttpClient.HttpClient, httpClient),
          Scope.provide(runScope),
          Effect.matchEffect({
            onFailure: (error) => finalizeRun(error.message),
            onSuccess: (exit) => finalizeRun(exit.reason, Option.getOrUndefined(exit.code)),
          }),
          Effect.ensuring(Scope.close(runScope, Exit.void).pipe(Effect.ignore)),
        );

        const fiber = yield* Effect.forkIn(program, parentScope);
        yield* updateActiveRun(runId, (run) => ({
          ...run,
          fiber: Option.some(fiber),
        }));
      }),
    ),
  ).pipe(Effect.withSpan("desktop.backendInstance.start", { attributes: { id: spec.id } }));

  const scheduleRestart = Effect.fn("desktop.backendInstance.scheduleRestart")(function* (
    reason: string,
    exitCode?: number,
  ) {
    const scheduled = yield* SubscriptionRef.modify(state, (latest) => {
      if (!latest.desiredRunning || Option.isSome(latest.restartFiber)) {
        return [Option.none<{ delay: Duration.Duration; attempt: number }>(), latest] as const;
      }

      const attempt = latest.restartAttempt + 1;
      return [
        Option.some({ delay: backendRestartDelay(latest.restartAttempt), attempt }),
        { ...latest, restartAttempt: attempt },
      ] as const;
    });

    yield* Option.match(scheduled, {
      onNone: () => Effect.void,
      onSome: Effect.fn("desktop.backendInstance.scheduleRestartFiber")(function* ({
        delay,
        attempt,
      }) {
        yield* logInstanceError("backend exited unexpectedly; restart scheduled", {
          reason,
          delayMs: Duration.toMillis(delay),
        });
        // Retrying cannot fix a policy refusal, so say so at once; retries still pick up a fix.
        const managedPolicy = exitCode === SERVER_EXIT_CODE_MANAGED_POLICY;
        if (managedPolicy || attempt >= STARTUP_FAILURE_RESTART_ATTEMPTS) {
          yield* reportStartupFailure(`The server could not start (${reason}).`, managedPolicy);
        }
        const restartFiber = yield* Effect.forkIn(
          Effect.sleep(delay).pipe(
            Effect.andThen(
              SubscriptionRef.modify(state, (latest) => {
                const shouldRestart = latest.desiredRunning;
                return [
                  shouldRestart,
                  {
                    ...latest,
                    restartFiber: Option.none(),
                  },
                ] as const;
              }),
            ),
            Effect.flatMap((shouldRestart) => (shouldRestart ? start : Effect.void)),
            Effect.catchCause((cause) =>
              logInstanceError("desktop backend restart fiber failed", {
                cause: Cause.pretty(cause),
              }),
            ),
          ),
          parentScope,
        );
        yield* SubscriptionRef.update(state, (latest) =>
          Option.isNone(latest.restartFiber)
            ? {
                ...latest,
                restartFiber: Option.some(restartFiber),
              }
            : latest,
        );
      }),
    });
  });

  const stop = Effect.fn("desktop.backendInstance.stop")(function* (options?: {
    readonly timeout?: Duration.Duration;
  }) {
    const { active, restartFiber, notifyShutdown } = yield* mutex.withPermits(1)(
      Effect.gen(function* () {
        const result = yield* SubscriptionRef.modify(state, (latest) => {
          const active = Option.map(latest.active, (run) =>
            run.exitObserved ? run : { ...run, stopRequested: true },
          );
          return [
            {
              active,
              restartFiber: latest.restartFiber,
              notifyShutdown: latest.ready,
            },
            {
              ...latest,
              desiredRunning: false,
              ready: false,
              active,
              restartFiber: Option.none<Fiber.Fiber<void, never>>(),
            },
          ] as const;
        });
        return result;
      }),
    );

    if (notifyShutdown) {
      yield* (spec.onShutdown?.() ?? Effect.void).pipe(Effect.ignore);
    }
    yield* Option.match(restartFiber, {
      onNone: () => Effect.void,
      onSome: (fiber) => Fiber.interrupt(fiber).pipe(Effect.asVoid),
    });
    yield* Option.match(active, {
      onNone: () => Effect.void,
      // The enclosing operation owns the span for this inline handler.
      onSome: Effect.fnUntraced(function* (run) {
        yield* closeRun(run, parentScope, options);
        const cleanup = yield* mutex.withPermits(1)(
          SubscriptionRef.modify(
            state,
            (
              latest,
            ): readonly [
              {
                readonly needsCleanup: boolean;
                readonly shouldStart: boolean;
              },
              BackendManagerState,
            ] => {
              const current = Option.getOrUndefined(latest.active);
              if (current?.id !== run.id) {
                return [
                  {
                    needsCleanup: false,
                    shouldStart:
                      latest.desiredRunning &&
                      Option.isNone(latest.active) &&
                      Option.isNone(latest.restartFiber),
                  },
                  latest,
                ];
              }
              return [
                {
                  needsCleanup: true,
                  shouldStart: latest.desiredRunning,
                },
                {
                  ...latest,
                  active: Option.none<ActiveBackendRun>(),
                },
              ];
            },
          ),
        );
        if (cleanup.needsCleanup) {
          yield* desktopTelemetryPublisher.removeControlSource(spec.id);
          yield* backendOutputLog.discardSession;
        }
        if (cleanup.shouldStart) {
          yield* start;
        }
      }),
    });
  });

  const waitForReady = Effect.fn("DesktopBackendManager.waitForReady")(
    (timeout: Duration.Duration) =>
      SubscriptionRef.changes(state).pipe(
        Stream.filter((current) => current.ready || !current.desiredRunning),
        Stream.map((current) => current.ready),
        Stream.runHead,
        Effect.timeoutOption(timeout),
        Effect.map(
          (result) => Option.isSome(result) && Option.getOrElse(result.value, () => false),
        ),
      ),
  );

  yield* Effect.addFinalizer(() => stop());

  return {
    id: spec.id,
    label: spec.label,
    start,
    stop,
    currentConfig,
    snapshot,
    waitForReady,
  } satisfies DesktopBackendInstance;
});
