import * as NodeCrypto from "@effect/platform-node/NodeCrypto";
import * as NodeServices from "@effect/platform-node/NodeServices";
import { assert, describe, it } from "@effect/vitest";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Deferred from "effect/Deferred";
import * as Fiber from "effect/Fiber";
import { TestClock } from "effect/testing";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";

import * as Net from "@t3tools/shared/Net";
import { makeNativeServerConfig } from "../native/localServer.ts";
import * as DesktopNetworkInterfaces from "./DesktopNetworkInterfaces.ts";
import { resolveNativeWslHost } from "../native/windowsHost.ts";
import * as DesktopEnvironment from "../app/DesktopEnvironment.ts";
import * as DesktopManagedPolicy from "../app/DesktopManagedPolicy.ts";
import * as DesktopBackendConfiguration from "./DesktopBackendConfiguration.ts";
import * as DesktopConfig from "../app/DesktopConfig.ts";
import * as DesktopAppSettings from "../settings/DesktopAppSettings.ts";
import * as DesktopWslEnvironment from "../wsl/DesktopWslEnvironment.ts";
import * as DesktopWslServerTree from "../wsl/DesktopWslServerTree.ts";

const resolveWsl = Effect.fn("test.resolveWsl")(function* (target: {
  port: number;
  distro: string | null;
}) {
  return yield* DesktopBackendConfiguration.resolveWslStartConfig({
    ...target,
    bootstrapToken: "token",
    bootstrapSecret: "secret",
    hostScope: yield* DesktopBackendConfiguration.readOrCreateHostScope(),
    observabilitySettings: yield* DesktopBackendConfiguration.readBackendObservabilitySettings,
  });
});

function layerEnvironment(
  baseDir: string,
  options?: {
    readonly appPath?: string;
    readonly dirname?: string;
    readonly isPackaged?: boolean;
    readonly devServerUrl?: string;
    readonly platform?: NodeJS.Platform;
    readonly resourcesPath?: string;
    readonly appVersion?: string;
    readonly processArch?: NodeJS.Architecture;
    readonly otlpTracesUrl?: string;
    readonly otlpMetricsUrl?: string;
    readonly otlpLogsUrl?: string;
  },
) {
  return DesktopEnvironment.layer({
    dirname: options?.dirname ?? "/repo/apps/desktop/src",
    homeDirectory: baseDir,
    platform: options?.platform ?? "darwin",
    processArch: options?.processArch ?? "x64",
    appVersion: options?.appVersion ?? "1.2.3",
    appPath: options?.appPath ?? "/repo",
    isPackaged: options?.isPackaged ?? true,
    resourcesPath: options?.resourcesPath ?? "/missing/resources",
    runningUnderArm64Translation: false,
  }).pipe(
    Layer.provide(
      Layer.mergeAll(
        NodeServices.layer,
        NodeCrypto.layer,
        DesktopConfig.layerTest({
          T3CODE_HOME: baseDir,
          T3CODE_PORT: "9999",
          T3CODE_MODE: "desktop",
          T3CODE_DESKTOP_LAN_HOST: "192.168.1.50",
          VITE_DEV_SERVER_URL: options?.devServerUrl,
          T3CODE_OTLP_TRACES_URL: options?.otlpTracesUrl,
          T3CODE_OTLP_METRICS_URL: options?.otlpMetricsUrl,
          T3CODE_OTLP_LOGS_URL: options?.otlpLogsUrl,
        }),
      ),
    ),
  );
}

interface PackagedWslHarnessContext {
  readonly baseDir: string;
  readonly archivePath: string;
  readonly hashPath: string;
  readonly archiveHash: string;
  readonly mountedAppRoot: string;
  readonly mountedEntryPath: string;
}

const withPackagedWslHarness = <A, E, R>(
  input: {
    readonly archiveHash: string;
    readonly wsl: (
      context: PackagedWslHarnessContext,
    ) => DesktopWslEnvironment.DesktopWslEnvironmentTestStub;
    readonly forbidFallback?: string;
    readonly cleanupLegacy?: Effect.Effect<void>;
    readonly forbidCleanup?: string;
  },
  effect: (
    context: PackagedWslHarnessContext,
  ) => Effect.Effect<
    A,
    E,
    | R
    | FileSystem.FileSystem
    | Path.Path
    | DesktopEnvironment.DesktopEnvironment
    | DesktopWslEnvironment.DesktopWslEnvironment
    | DesktopWslServerTree.DesktopWslServerTree
  >,
) =>
  Effect.gen(function* () {
    const fileSystem = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const baseDir = yield* fileSystem.makeTempDirectoryScoped({
      prefix: "t3-desktop-backend-config-test-",
    });
    const archivePath = path.join(baseDir, "wsl-runtime.tar.gz");
    const hashPath = `${archivePath}.sha256`;
    const mountedAppRoot = "/mnt/c/app.asar.unpacked";
    const mountedEntryPath = path.join(baseDir, "app.asar.unpacked/apps/server/dist/bin.mjs");
    yield* fileSystem.makeDirectory(path.dirname(mountedEntryPath), { recursive: true });
    yield* fileSystem.writeFileString(mountedEntryPath, "");
    yield* fileSystem.writeFileString(archivePath, "archive");
    yield* fileSystem.writeFileString(hashPath, `${input.archiveHash}\n`);

    const context = {
      baseDir,
      archivePath,
      hashPath,
      archiveHash: input.archiveHash,
      mountedAppRoot,
      mountedEntryPath,
    } satisfies PackagedWslHarnessContext;
    const layerServerTree = input.forbidFallback
      ? Layer.succeed(
          DesktopWslServerTree.DesktopWslServerTree,
          DesktopWslServerTree.DesktopWslServerTree.of({
            ensure: Effect.die(input.forbidFallback),
            cleanupLegacy: input.forbidCleanup
              ? Effect.die(input.forbidCleanup)
              : (input.cleanupLegacy ?? Effect.void),
          }),
        )
      : DesktopWslServerTree.layerTest({
          result: { ok: true, root: path.join(baseDir, "app.asar.unpacked") },
          cleanupLegacy: input.cleanupLegacy ?? Effect.void,
        });

    return yield* effect(context).pipe(
      Effect.provide(
        layerServerTree.pipe(
          Layer.provideMerge(
            DesktopWslEnvironment.layerTest({
              isAvailable: true,
              distros: [{ name: "Ubuntu", isDefault: true, version: 2 }],
              windowsToWslPath: () => Option.some(mountedAppRoot),
              getDistroIp: () => Option.some("172.27.0.99"),
              ...input.wsl(context),
            }),
          ),
          Layer.provideMerge(
            layerEnvironment(baseDir, {
              appPath: baseDir,
              platform: "win32",
              resourcesPath: baseDir,
            }),
          ),
        ),
      ),
    );
  }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer)));

describe("DesktopBackendConfiguration", () => {
  it.effect(
    "starts packaged Rune with the saved network settings and keeps checkout servers isolated",
    () =>
      Effect.gen(function* () {
        const settings = {
          ...DesktopAppSettings.DEFAULT_DESKTOP_SETTINGS,
          serverExposureMode: "network-accessible" as const,
          tailscaleServeEnabled: true,
          tailscaleServePort: 8443,
        };
        const launch = (packaged: boolean, managed = false, devPort?: number) =>
          makeNativeServerConfig(
            {
              packaged,
              version: "1.2.3",
              t3Home: "/profile",
              entryPath: "/server/bin.mjs",
              cwd: "/server",
              ...(devPort === undefined ? {} : { dev: { port: devPort } }),
            },
            settings,
            "token",
            "secret",
          ).pipe(
            Effect.provide(
              DesktopManagedPolicy.layerTest(
                managed
                  ? { schemaVersion: 1, organization: "Example", limits: { clients: ["desktop"] } }
                  : undefined,
              ),
            ),
          );
        for (const [config, port, host, serve, servePort] of [
          [yield* launch(true), 4099, "0.0.0.0", true, 8443],
          [yield* launch(true, true), 4099, "127.0.0.1", false, 8443],
          [yield* launch(false), 5100, "127.0.0.1", false, 443],
          [yield* launch(false, false, 6200), 6200, "127.0.0.1", false, 443],
        ] as const) {
          assert.equal(config.httpBaseUrl.href, `http://127.0.0.1:${port}/`);
          assert.include(config.bootstrap, {
            port,
            host,
            tailscaleServeEnabled: serve,
            tailscaleServePort: servePort,
          });
        }
      }).pipe(
        Effect.provide(
          DesktopConfig.layerTest({ T3CODE_PORT: "4099", T3CODE_DESKTOP_LAN_HOST: "fixture.lan" }),
        ),
        Effect.provideService(DesktopNetworkInterfaces.DesktopNetworkInterfaces, {
          read: Effect.succeed({}),
        }),
        Effect.provide(
          Layer.mock(Net.NetService, { reserveLoopbackPort: () => Effect.succeed(5100) }),
        ),
      ),
  );

  it.effect("retries a transient WSL mount and isolates native development homes by checkout", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const root = yield* fs.makeTempDirectoryScoped();
      const source = path.join(root, "apps/server/src");
      yield* fs.makeDirectory(source, { recursive: true });
      yield* fs.writeFileString(path.join(source, "bin.ts"), "");
      const failed = yield* Deferred.make<void>();
      let attempts = 0;
      const serverTree = DesktopWslServerTree.DesktopWslServerTree.of({
        ensure: Effect.gen(function* () {
          if (++attempts === 1) {
            yield* Deferred.succeed(failed, undefined);
            return { ok: false, fatal: false, reason: "mount unavailable" } as const;
          }
          return { ok: true, root, serverEntry: "src/bin.ts" } as const;
        }),
        cleanupLegacy: Effect.void,
      });
      const launch = (home: string) =>
        resolveNativeWslHost(
          { port: 5000, desktopBootstrapToken: "token", desktopBootstrapSecret: "secret" },
          "Ubuntu",
          "scope",
        ).pipe(
          Effect.provideService(DesktopWslServerTree.DesktopWslServerTree, serverTree),
          Effect.provide([
            DesktopWslEnvironment.layerTest({
              isAvailable: true,
              distros: [{ name: "Ubuntu", version: 2, isDefault: true }],
              windowsToWslPath: () => Option.some("/mnt/c/repo"),
              ensureNodePty: () => ({
                ok: true,
                nodePath: "/usr/bin/node",
                resolvedPath: "/usr/bin",
              }),
            }),
            layerEnvironment(home, { isPackaged: false, platform: "win32" }),
          ]),
          Effect.map((result) => {
            assert.isDefined(result);
            assert.equal(result.config.runningDistro, "Ubuntu");
            assert.include(result.config.args, "/mnt/c/repo/apps/server/src/bin.ts");
            assert.equal(result.config.bootstrap.desktopTelemetryControlFd, 1);
            assert.equal(result.config.args.at(-2), "--base-dir");
            return result.config.args.at(-1);
          }),
        );
      const first = yield* Effect.forkScoped(launch(root));
      yield* Deferred.await(failed);
      yield* TestClock.adjust("500 millis");
      const firstHome = yield* Fiber.join(first);
      assert.equal(attempts, 2);
      assert.match(firstHome!, /^~\/\.t3\/rune-dev\/[a-f0-9]+$/);
      assert.notEqual(yield* launch(path.join(root, "other")), firstHome);
      assert.equal(yield* launch(root), firstHome);
    }).pipe(Effect.scoped, Effect.provide([NodeServices.layer, NodeCrypto.layer])),
  );

  it.effect("resolveWsl pins a default-tracking run to the concrete default distro", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "t3-desktop-backend-config-test-",
      });
      const entryPath = path.join(baseDir, "apps/server/dist/bin.mjs");
      yield* fileSystem.makeDirectory(path.dirname(entryPath), { recursive: true });
      yield* fileSystem.writeFileString(entryPath, "");

      const observedDistros: Array<string | null> = [];
      const config = yield* resolveWsl({ port: 5000, distro: null }).pipe(
        Effect.provide(
          DesktopWslServerTree.layerTest().pipe(
            Layer.provideMerge(
              DesktopWslEnvironment.layerTest({
                isAvailable: true,
                distros: [
                  { name: "Debian", isDefault: false, version: 2 },
                  { name: "Ubuntu", isDefault: true, version: 2 },
                ],
                windowsToWslPath: (distro) => {
                  observedDistros.push(distro);
                  return Option.some("/repo");
                },
                ensureNodePty: (distro) => {
                  observedDistros.push(distro);
                  return { ok: true, nodePath: "/usr/bin/node", resolvedPath: "/usr/bin:/bin" };
                },
                getDistroIp: (distro) => {
                  observedDistros.push(distro);
                  return Option.some("172.27.0.99");
                },
              }),
            ),
            Layer.provideMerge(
              layerEnvironment(baseDir, {
                appPath: baseDir,
                platform: "win32",
                resourcesPath: baseDir,
              }),
            ),
          ),
        ),
      );

      assert.equal(config.runningDistro, "Ubuntu");
      assert.deepEqual(config.args.slice(0, 5), ["-d", "Ubuntu", "--cd", "~", "--exec"]);
      assert.deepEqual(observedDistros, ["Ubuntu", "Ubuntu", "Ubuntu"]);
      assert.isTrue(Option.isNone(config.preflightFailure));
    }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );

  it.effect("resolveWsl launches a packaged backend from the WSL-local runtime cache", () => {
    const observedArchives: Array<{
      windowsArchivePath: string;
      runtimeId: string;
      sha256: string;
    }> = [];
    let probeCount = 0;
    let legacyCleanupCount = 0;
    const linuxAppRoot = "/home/test/.t3/wsl-runtime/1.2.3-x64";
    const resolvedPath = "/home/test/.local/bin:/usr/bin:/bin";

    return withPackagedWslHarness(
      {
        archiveHash: "a".repeat(64),
        forbidFallback: "A valid WSL archive must not extract the Windows fallback",
        cleanupLegacy: Effect.sync(() => {
          legacyCleanupCount += 1;
        }),
        wsl: () => ({
          prepareRuntime: (_distro, archive) => {
            observedArchives.push({
              windowsArchivePath: archive.windowsPath,
              runtimeId: archive.runtimeId,
              sha256: archive.sha256,
            });
            return { ok: true, linuxAppRoot };
          },
          probeRuntime: () => {
            probeCount += 1;
            return { ok: true, resolvedPath };
          },
          ensureNodePty: () => {
            throw new Error("the staged runtime must not probe for node-pty");
          },
        }),
      },
      ({ archiveHash, archivePath, baseDir }) =>
        Effect.gen(function* () {
          const path = yield* Path.Path;

          const config = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });

          assert.deepEqual(observedArchives, [
            {
              windowsArchivePath: archivePath,
              runtimeId: `sha256-${archiveHash}`,
              sha256: archiveHash,
            },
          ]);
          assert.equal(probeCount, 1);
          assert.equal(
            config.entryPath,
            path.join(baseDir, "server.asar/apps/server/dist/bin.mjs"),
          );
          assert.deepEqual(config.args, [
            "-d",
            "Ubuntu",
            "--cd",
            "~",
            "--exec",
            "env",
            `PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:${resolvedPath}`,
            `${linuxAppRoot}/t3`,
            "--bootstrap-fd",
            "0",
          ]);
          assert.notInclude(config.args, "/usr/bin/node");
          assert.equal(config.wslRuntimeId, `sha256-${archiveHash}`);
          assert.equal(legacyCleanupCount, 1);
          assert.isTrue(Option.isNone(config.preflightFailure));
        }),
    );
  });

  it.effect("resolveWsl changes the cache id when the packaged archive changes", () => {
    const firstHash = "a".repeat(64);
    const secondHash = "b".repeat(64);
    const observedRuntimeIds: string[] = [];
    return withPackagedWslHarness(
      {
        archiveHash: firstHash,
        wsl: () => ({
          prepareRuntime: (_distro, archive) => {
            observedRuntimeIds.push(archive.runtimeId);
            return { ok: true, linuxAppRoot: `/runtime/${archive.runtimeId}` };
          },
          ensureNodePty: () => ({
            ok: true,
            nodePath: "/usr/bin/node",
            resolvedPath: "/usr/bin:/bin",
          }),
        }),
      },
      ({ hashPath, mountedAppRoot }) =>
        Effect.gen(function* () {
          const fileSystem = yield* FileSystem.FileSystem;

          const first = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });
          yield* fileSystem.writeFileString(hashPath, secondHash);
          const second = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });
          yield* fileSystem.writeFileString(hashPath, "not-a-sha256");
          const invalidIdentity = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });

          assert.deepEqual(observedRuntimeIds, [`sha256-${firstHash}`, `sha256-${secondHash}`]);
          assert.equal(first.wslRuntimeId, observedRuntimeIds[0]);
          assert.include(first.args, `/runtime/sha256-${firstHash}/t3`);
          assert.equal(second.wslRuntimeId, observedRuntimeIds[1]);
          assert.include(second.args, `/runtime/sha256-${secondHash}/t3`);
          assert.isUndefined(invalidIdentity.wslRuntimeId);
          assert.include(invalidIdentity.args, "/usr/bin/node");
          assert.include(invalidIdentity.args, `${mountedAppRoot}/apps/server/dist/bin.mjs`);
        }),
    );
  });

  it.effect("resolveWsl falls back to the mounted runtime when archive staging fails", () => {
    const observedNodePtyRoots: string[] = [];
    return withPackagedWslHarness(
      {
        archiveHash: "b".repeat(64),
        wsl: () => ({
          prepareRuntime: () => ({ ok: false, reason: "archive is corrupt" }),
          invalidateRuntime: () => Effect.die("failed staging must not invalidate a runtime"),
          ensureNodePty: (_distro, root) => {
            observedNodePtyRoots.push(root);
            return { ok: true, nodePath: "/usr/bin/node", resolvedPath: "/usr/bin:/bin" };
          },
        }),
      },
      ({ mountedAppRoot, mountedEntryPath }) =>
        Effect.gen(function* () {
          const config = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });

          assert.deepEqual(observedNodePtyRoots, [mountedAppRoot]);
          assert.equal(config.entryPath, mountedEntryPath);
          assert.include(config.args, "/usr/bin/node");
          assert.include(config.args, `${mountedAppRoot}/apps/server/dist/bin.mjs`);
          assert.isUndefined(config.wslRuntimeId);
          assert.isTrue(Option.isNone(config.preflightFailure));
        }),
    );
  });

  it.effect("resolveWsl retires a staged runtime whose launch probe fails", () => {
    const archiveHash = "c".repeat(64);
    const stagedAppRoot = `/home/test/.t3/wsl-runtime/sha256-${archiveHash}`;
    let probeCount = 0;
    const observedNodePtyRoots: string[] = [];
    const invalidatedRuntimeIds: string[] = [];
    return withPackagedWslHarness(
      {
        archiveHash,
        wsl: () => ({
          prepareRuntime: () => ({ ok: true, linuxAppRoot: stagedAppRoot }),
          invalidateRuntime: (_distro, runtimeId) =>
            Effect.sync(() => {
              invalidatedRuntimeIds.push(runtimeId);
            }),
          probeRuntime: () => {
            probeCount += 1;
            return { ok: false, reason: "WSL login-shell PATH could not be resolved" };
          },
          ensureNodePty: (_distro, root) => {
            observedNodePtyRoots.push(root);
            return { ok: true, nodePath: "/usr/bin/node", resolvedPath: "/usr/bin:/bin" };
          },
        }),
      },
      ({ mountedAppRoot, mountedEntryPath }) =>
        Effect.gen(function* () {
          const config = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });

          assert.equal(probeCount, 1);
          assert.deepEqual(observedNodePtyRoots, [mountedAppRoot]);
          assert.include(config.args, "/usr/bin/node");
          assert.include(config.args, `${mountedAppRoot}/apps/server/dist/bin.mjs`);
          assert.notInclude(config.args, `${stagedAppRoot}/t3`);
          assert.equal(config.entryPath, mountedEntryPath);
          assert.isUndefined(config.wslRuntimeId);
          assert.isTrue(Option.isNone(config.preflightFailure));
          assert.deepEqual(invalidatedRuntimeIds, [`sha256-${archiveHash}`]);
        }),
    );
  });

  it.effect("resolveWsl keeps WSL retryable when the mounted fallback fails transiently", () => {
    const stagedAppRoot = "/home/test/.t3/wsl-runtime/cache";
    const invalidatedRuntimeIds: string[] = [];
    return withPackagedWslHarness(
      {
        archiveHash: "f".repeat(64),
        wsl: () => ({
          prepareRuntime: () => ({ ok: true, linuxAppRoot: stagedAppRoot }),
          invalidateRuntime: (_distro, runtimeId) =>
            Effect.sync(() => {
              invalidatedRuntimeIds.push(runtimeId);
            }),
          probeRuntime: () => ({ ok: false, reason: "WSL login-shell PATH could not be resolved" }),
          ensureNodePty: () => ({
            ok: false,
            reason: "WSL backend preflight timed out while probing for Node.js.",
            fatal: false,
          }),
        }),
      },
      () =>
        Effect.gen(function* () {
          const config = yield* resolveWsl({ port: 5000, distro: "Ubuntu" });
          const failure = Option.getOrThrow(config.preflightFailure);

          assert.isFalse(failure.fatal);
          assert.equal(failure.retryLimit, 12);
          assert.include(failure.reason, "timed out");
          assert.deepEqual(invalidatedRuntimeIds, []);
        }),
    );
  });

  it.effect(
    "resolveWsl preserves inherited PATH with quote-sensitive values as separate args",
    () =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        const baseDir = yield* fileSystem.makeTempDirectoryScoped({
          prefix: "t3-desktop-backend-config-test-",
        });
        const entryPath = path.join(baseDir, "apps/server/dist/bin.mjs");
        yield* fileSystem.makeDirectory(path.dirname(entryPath), { recursive: true });
        yield* fileSystem.writeFileString(entryPath, "");

        const nodePath = "/home/test user's/.nvm/versions/node/v22.0.0/bin/node";
        const linuxAppRoot = "/tmp/t3 code's launch";
        const linuxEntryPath = `${linuxAppRoot}/apps/server/dist/bin.mjs`;
        const resolvedPath = "/home/test user/bin:/opt/test's tools/bin:/usr/bin:/bin";
        const devServerUrl = "http://127.0.0.1:5733/dev%20assets/?label=hello%20world";
        const config = yield* resolveWsl({ port: 5000, distro: "Ubuntu" }).pipe(
          Effect.provide(
            DesktopWslServerTree.layerTest().pipe(
              Layer.provideMerge(
                DesktopWslEnvironment.layerTest({
                  isAvailable: true,
                  distros: [{ name: "Ubuntu", isDefault: true, version: 2 }],
                  windowsToWslPath: () => Option.some(linuxAppRoot),
                  ensureNodePty: () => ({ ok: true, nodePath, resolvedPath }),
                  getDistroIp: () => Option.some("172.27.0.99"),
                }),
              ),
              Layer.provideMerge(
                layerEnvironment(baseDir, {
                  appPath: baseDir,
                  devServerUrl,
                  isPackaged: true,
                  platform: "win32",
                  resourcesPath: baseDir,
                }),
              ),
            ),
          ),
        );

        assert.equal(config.bootstrapDelivery, "stdin");
        assert.deepEqual(config.args, [
          "-d",
          "Ubuntu",
          "--cd",
          "~",
          "--exec",
          "env",
          "PATH=/home/test user's/.nvm/versions/node/v22.0.0/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/home/test user/bin:/opt/test's tools/bin:/usr/bin:/bin",
          nodePath,
          linuxEntryPath,
          "--bootstrap-fd",
          "0",
          "--dev-url",
          devServerUrl,
        ]);
        assert.notInclude(config.args, "bash");
        assert.notInclude(config.args, "/bin/sh");
        assert.notInclude(config.args, "-c");
        assert.isTrue(Option.isNone(config.preflightFailure));
      }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );

  it.effect("keeps the Windows and WSL host scope across restarts", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const home = yield* fs.makeTempDirectoryScoped();
      const launch = DesktopBackendConfiguration.readOrCreateHostScope().pipe(
        Effect.provide(layerEnvironment(home, { platform: "win32" })),
      );
      const first = yield* launch;
      assert.match(first, /^[0-9a-f]{32}$/);
      assert.equal(yield* launch, first);
    }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );

  it.effect("resolveWsl carries the kill switch and resource attributes into the distro", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const baseDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "t3-desktop-backend-config-test-",
      });

      yield* Effect.gen(function* () {
        const config = yield* resolveWsl({ port: 5050, distro: null });

        assert.equal(config.env.OTEL_SDK_DISABLED, "true");
        assert.include((config.env.WSLENV ?? "").split(":"), "OTEL_SDK_DISABLED");
        assert.equal(config.env.T3CODE_TELEMETRY_ENABLED, "false");
        assert.include((config.env.WSLENV ?? "").split(":"), "T3CODE_TELEMETRY_ENABLED");
        assert.equal(config.env.OTEL_RESOURCE_ATTRIBUTES, "deployment.environment.name=staging");
        assert.include((config.env.WSLENV ?? "").split(":"), "OTEL_RESOURCE_ATTRIBUTES");
      }).pipe(
        Effect.provide(
          DesktopWslServerTree.layerTest().pipe(
            Layer.provideMerge(
              DesktopWslEnvironment.layerTest({
                isAvailable: true,
                windowsToWslPath: () => Option.some("/mnt/c/repo/apps/server/src/index.ts"),
                getDistroIp: () => Option.some("172.27.0.99"),
              }),
            ),
            Layer.provideMerge(layerEnvironment(baseDir, { platform: "win32" })),
          ),
        ),
        Effect.provideService(HostProcess.Environment, {
          OTEL_SDK_DISABLED: "true",
          T3CODE_TELEMETRY_ENABLED: "false",
          OTEL_RESOURCE_ATTRIBUTES: "deployment.environment.name=staging",
        }),
      );
    }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );

  it.effect("resolveWsl preserves existing WSLENV entries when forwarding backend secrets", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const baseDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "t3-desktop-backend-config-test-",
      });

      yield* Effect.gen(function* () {
        const config = yield* resolveWsl({ port: 5050, distro: null });

        assert.equal(config.executablePath, "wsl.exe");
        assert.equal(config.bootstrap.port, 5050);
        assert.equal(config.bootstrap.host, "0.0.0.0");
        assert.equal(config.bootstrap.tailscaleServeEnabled, false);
        assert.notProperty(config.bootstrap, "desktopTelemetryFd");
        assert.notProperty(config.bootstrap, "resourceMonitorPath");
        assert.equal(config.httpBaseUrl.href, "http://172.27.0.99:5050/");
        assert.equal(config.env.OPENAI_API_KEY, "openai-key");
        assert.equal(config.env.ANTHROPIC_API_KEY, "anthropic-key");
        assert.equal(config.env.T3CODE_OTLP_PROTOCOL, "http/protobuf");
        assert.equal(
          config.env.WSLENV,
          "GOPATH/p:OPENAI_API_KEY/u:EMPTY::AZURE_DEVOPS_EXT_PAT/u:T3CODE_DESKTOP_HOST_SCOPE:ANTHROPIC_API_KEY:T3CODE_OTLP_HEADERS:T3CODE_OTLP_PROTOCOL",
        );
      }).pipe(
        Effect.provide(
          DesktopWslServerTree.layerTest().pipe(
            Layer.provideMerge(
              DesktopWslEnvironment.layerTest({
                isAvailable: true,
                windowsToWslPath: () => Option.some("/mnt/c/repo/apps/server/src/index.ts"),
                getDistroIp: () => Option.some("172.27.0.99"),
              }),
            ),
            Layer.provideMerge(layerEnvironment(baseDir, { platform: "win32" })),
          ),
        ),
        Effect.provideService(HostProcess.Environment, {
          WSLENV: "GOPATH/p:OPENAI_API_KEY/u:EMPTY::AZURE_DEVOPS_EXT_PAT/u",
          OPENAI_API_KEY: "openai-key",
          ANTHROPIC_API_KEY: "anthropic-key",
          T3CODE_OTLP_HEADERS: 'authorization="Bearer%20my-token"',
          T3CODE_OTLP_PROTOCOL: "http/protobuf",
        }),
      );
    }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );

  it.effect("resolveWsl marks a missing packaged server entry as fatal", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const baseDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "t3-desktop-backend-config-test-",
      });

      yield* Effect.gen(function* () {
        const config = yield* resolveWsl({ port: 5050, distro: "Ubuntu" });
        const failure = Option.getOrThrow(config.preflightFailure);

        assert.isTrue(failure.fatal);
        assert.include(failure.reason, "missing server entry");
      }).pipe(
        Effect.provide(
          DesktopWslServerTree.layerTest().pipe(
            Layer.provideMerge(
              DesktopWslEnvironment.layerTest({
                isAvailable: true,
                distros: [{ name: "Ubuntu", isDefault: true, version: 2 }],
              }),
            ),
            Layer.provideMerge(layerEnvironment(baseDir, { platform: "win32" })),
          ),
        ),
      );
    }).pipe(Effect.scoped, Effect.provide(Layer.mergeAll(NodeServices.layer, NodeCrypto.layer))),
  );
});
