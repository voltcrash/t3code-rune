import "vite-plus/test/config";

import { defaultInclude, defineConfig } from "vite-plus";

const domTests = ["src/preview/PickPreload.test.ts"];

export default defineConfig({
  run: {
    tasks: {
      build: {
        command: "node scripts/build-rune.mjs",
        dependsOn: ["t3#build"],
        cache: false,
      },
      "build:bundle": {
        command: "node scripts/bundle-rune-worker.mjs",
        dependsOn: ["t3#build"],
        cache: false,
      },
      dev: {
        command: "node scripts/launch-rune.mjs --watch",
        cache: false,
      },
      // A local tsbuildinfo is leftover build state, not a typecheck input.
      typecheck: {
        command: "effect-tsgo tsrs --noEmit",
        cache: { input: [{ auto: true }, "!*.tsbuildinfo"] },
      },
    },
  },
  test: {
    // The Windows lane runs workspace suites concurrently; filesystem-heavy
    // desktop integration tests can exceed Vitest's 5 second default there.
    testTimeout: 15_000,
    projects: [
      {
        extends: true,
        test: {
          name: "unit",
          include: [...defaultInclude, "../../native/kde-snap-shot/src/*.test.ts"],
          exclude: ["**/node_modules/**", ...domTests],
          setupFiles: [
            "./src/testing/electron.ts",
            "../../packages/shared/src/testing/longTempDir.ts",
            "../../packages/shared/src/testing/providerEnv.ts",
            "../../packages/shared/src/testing/gitEnv.ts",
          ],
        },
      },
      {
        extends: true,
        test: {
          name: "dom",
          include: domTests,
          // Guest preloads run in the page; these tests drive them against a DOM.
          environment: "jsdom",
          setupFiles: [
            "../../packages/shared/src/testing/providerEnv.ts",
            "../../packages/shared/src/testing/gitEnv.ts",
          ],
        },
      },
    ],
  },
});
