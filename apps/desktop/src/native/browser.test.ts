import { layerRemoteHttpClient } from "@t3tools/client-runtime/rpc";
import { EnvironmentId, WS_METHODS } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Stream from "effect/Stream";
import { expect, it, vi } from "vite-plus/test";
import { openBrowserPanel } from "./browser.ts";

const { openRpc } = vi.hoisted(() => ({ openRpc: vi.fn() }));
vi.mock("./environmentRpc.ts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./environmentRpc.ts")>()),
  openEnvironmentRpc: openRpc,
}));
vi.mock("@t3tools/client-runtime/state/deviceHubAccess", () => ({
  resolveDeviceHubAccess: () => Effect.never,
}));

it.each(["switch away and back", "close the tab", "revoke write access"])(
  "drops a queued page adjustment when you %s",
  async (change) => {
    const ready = Promise.withResolvers<void>();
    const entered = Promise.withResolvers<void>();
    const release = Promise.withResolvers<void>();
    const closing = Promise.withResolvers<void>();
    const closed = Promise.withResolvers<void>();
    const tabs = ["a", "b"].map((tabId) => ({
      tabId,
      runtime: "server",
      navStatus: { _tag: "Idle" },
    }));
    const adjust = vi.fn(() =>
      Effect.promise(async () => {
        entered.resolve();
        await release.promise;
        return tabs[0];
      }),
    );
    openRpc.mockReturnValue(
      Effect.succeed({
        [WS_METHODS.subscribePreviewEvents]: () => Stream.never,
        [WS_METHODS.previewList]: () => Effect.succeed({ revision: 1, sessions: tabs }),
        [WS_METHODS.previewAdjust]: adjust,
        [WS_METHODS.previewClose]: () =>
          Effect.promise(() => {
            closing.resolve();
            return closed.promise;
          }),
      }),
    );
    const session = {
      environmentId: EnvironmentId.make("browser"),
      httpBaseUrl: "https://example.test",
      label: "Browser",
      token: "fixture",
      browserWritable: true,
      http: layerRemoteHttpClient(globalThis.fetch),
    };
    const panel = openBrowserPanel(
      session,
      "thread",
      new AbortController().signal,
      (state) => {
        if (state.selected === "a") ready.resolve();
      },
      () => {},
    );
    try {
      await ready.promise;
      const first = panel.request({ action: "adjust", tab: "a", setting: "zoomIn" });
      await entered.promise;
      const queued = panel.request({ action: "adjust", tab: "a", setting: "cookies" });
      if (change === "switch away and back") {
        await panel.request({ action: "select", tab: "b" });
        await panel.request({ action: "select", tab: "a" });
      } else if (change === "close the tab") {
        void panel.request({ action: "close", tab: "a" });
        await closing.promise;
      } else {
        session.browserWritable = false;
      }
      release.resolve();
      await Promise.all([first, queued]);
      expect(adjust).toHaveBeenCalledTimes(1);
    } finally {
      closed.resolve();
      release.resolve();
      panel.stop();
    }
  },
);

it("confirms a rotated viewport even when the follow-up tab list fails", async () => {
  const ready = Promise.withResolvers<void>();
  const tab = {
    tabId: "a",
    runtime: "server",
    navStatus: { _tag: "Idle" },
    viewport: { _tag: "freeform", width: 600, height: 400 },
  };
  const resized = { ...tab, viewport: { _tag: "freeform", width: 400, height: 600 } };
  const list = vi
    .fn()
    .mockReturnValue(Effect.fail(new Error("offline")))
    .mockReturnValueOnce(Effect.succeed({ revision: 1, sessions: [tab] }));
  openRpc.mockReturnValue(
    Effect.succeed({
      [WS_METHODS.subscribePreviewEvents]: () => Stream.never,
      [WS_METHODS.previewList]: list,
      [WS_METHODS.previewResize]: () => Effect.succeed(resized),
    }),
  );
  const pages: unknown[] = [];
  const panel = openBrowserPanel(
    {
      environmentId: EnvironmentId.make("browser"),
      httpBaseUrl: "https://example.test",
      http: layerRemoteHttpClient(globalThis.fetch),
      label: "Browser",
      token: "fixture",
      browserWritable: true,
    },
    "thread",
    new AbortController().signal,
    (page) => {
      pages.push(page);
      if (page.selected === "a") ready.resolve();
    },
    () => {},
  );
  try {
    await ready.promise;
    await panel.request({ action: "viewport", tab: "a", change: { kind: "rotate" } });
    expect(pages).toContainEqual(
      expect.objectContaining({ deviceChange: "rotate", selected: "a" }),
    );
    expect(pages.at(-1)).toMatchObject({
      error: expect.any(String),
      tabs: [{ device: resized.viewport }],
    });
  } finally {
    panel.stop();
  }
});
