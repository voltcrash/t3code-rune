import {
  createPreviewStreamClient,
  PREVIEW_STREAM_BASE_PATH,
  previewStreamControlLabel,
  type PreviewStreamClient,
  type PreviewStreamControl,
  type PreviewStreamInput,
} from "@t3tools/client-runtime/preview/server-browser-stream";
import { resolveDeviceHubAccess } from "@t3tools/client-runtime/state/deviceHubAccess";
import {
  DEFAULT_PREVIEW_APPEARANCE,
  DEFAULT_PREVIEW_ZOOM_FACTOR,
  PREVIEW_ZOOM_LEVELS,
  type PreviewAdjustInput,
  type PreviewSessionSnapshot,
  ThreadId,
  WS_METHODS,
} from "@t3tools/contracts";
import { resolveAddressBarInput } from "@t3tools/shared/preview";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as Stream from "effect/Stream";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
import { BrowserDialog } from "./browserDialog.ts";
import { openEnvironmentRpc, preparedConnection } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

export function browserLabels() {
  return {
    title: m(msg("Browser")),
    address: m(msg("Search or enter URL")),
    add: m(msg("New tab")),
    closeTab: m(msg("Close tab")),
    close: m(msg("Close")),
    back: m(msg("Back")),
    forward: m(msg("Forward")),
    reload: m(msg("Reload")),
    take: m(msg("Take control")),
    release: m(msg("Release control")),
    retry: m(msg("Try again")),
    dialog: m(msg("Browser dialog")),
    response: m(msg("Dialog response")),
    dismiss: m(msg("Dismiss")),
    accept: m(msg("Accept")),
    recheckDialog: m(msg("Check pending dialog")),
    takeToRespond: m(msg("Take control to respond.")),
    menu: {
      title: m(msg("Preview menu")),
      reload: m(msg("Hard reload")),
      zoom: m(msg("Zoom")),
      zoomIn: m(msg("Zoom in")),
      zoomOut: m(msg("Zoom out")),
      reset: m(msg("Reset zoom")),
      appearance: m(msg("Appearance")),
      system: m(msg("System", { $context: "color scheme" })),
      light: m(msg("Light", { $context: "color scheme" })),
      dark: m(msg("Dark", { $context: "color scheme" })),
      cookies: m(msg("Clear cookies")),
      cache: m(msg("Clear cache")),
    },
  };
}

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type BrowserRequest =
  | { action: "add" | "navigate"; value: string; tab: string | null }
  | { action: "select"; tab: string }
  | { action: "close"; tab: string }
  | { action: "input"; tab: string; input: PreviewStreamInput }
  | { action: "recheckDialog"; tab: string; dialogId: number }
  | { action: "dialog"; tab: string; dialogId: number; accept: boolean; promptText?: string }
  | {
      action: "adjust";
      tab: string;
      setting:
        | "hardReload"
        | "zoomIn"
        | "zoomOut"
        | "resetZoom"
        | "system"
        | "light"
        | "dark"
        | "cookies"
        | "cache";
    }
  | { action: "retry" }
  | { action: "painted"; sequence: number };

function tabAdjustment(
  tab: PreviewSessionSnapshot,
  setting: Extract<BrowserRequest, { action: "adjust" }>["setting"],
): Omit<PreviewAdjustInput, "tabId" | "threadId"> | undefined {
  switch (setting) {
    case "hardReload":
      return { hardReload: true };
    case "cookies":
    case "cache":
      return { clear: setting };
    case "system":
    case "light":
    case "dark":
      return { colorScheme: setting };
    case "zoomIn":
    case "zoomOut":
    case "resetZoom": {
      const current = tab.zoomFactor ?? DEFAULT_PREVIEW_ZOOM_FACTOR;
      const next =
        setting === "resetZoom"
          ? DEFAULT_PREVIEW_ZOOM_FACTOR
          : PREVIEW_ZOOM_LEVELS[
              PREVIEW_ZOOM_LEVELS.indexOf(current) + (setting === "zoomIn" ? 1 : -1)
            ];
      return next !== undefined && current !== next ? { zoomFactor: next } : undefined;
    }
  }
}

function tabView(tab: PreviewSessionSnapshot) {
  const nav = tab.navStatus;
  return {
    id: tab.tabId,
    url: nav._tag === "Idle" ? "" : nav.url,
    title: nav._tag === "Idle" ? m(msg("New tab")) : nav.title || nav.url,
    zoom: Math.round(100 * (tab.zoomFactor ?? DEFAULT_PREVIEW_ZOOM_FACTOR)),
    scheme: tab.colorScheme ?? DEFAULT_PREVIEW_APPEARANCE,
    can_zoom_in: tab.zoomFactor !== PREVIEW_ZOOM_LEVELS.at(-1),
    can_zoom_out: tab.zoomFactor !== PREVIEW_ZOOM_LEVELS[0],
    back: tab.canGoBack,
    forward: tab.canGoForward,
    failure: nav._tag === "LoadFailed" ? nav.description : null,
  };
}

type Page = {
  tabs: ReturnType<typeof tabView>[];
  selected: string | null;
  dialog: BrowserDialog["current"];
  canWrite: boolean;
  controlled: boolean;
  canControl: boolean;
  status: string;
  viewport: readonly [number, number];
  busy: boolean;
  error: string | null;
};

export function openBrowserPanel(
  session: Pick<
    Session,
    "httpBaseUrl" | "http" | "label" | "token" | "environmentId" | "browserWritable"
  >,
  thread: string,
  signal: AbortSignal,
  publish: (page: Page) => void,
  paint: (tab: string, sequence: number, jpeg: ArrayBuffer) => void,
  address: (input: string) => string = resolveAddressBarInput,
) {
  const threadId = ThreadId.make(thread);
  const stopped = new AbortController();
  const lifetime = AbortSignal.any([signal, stopped.signal]);
  let service: Rpc | undefined;
  let tabs: PreviewSessionSnapshot[] = [];
  let selected: string | null = null;
  let stream: PreviewStreamClient | undefined;
  let attaching: AbortController | undefined;
  let checking: AbortController | undefined;
  let adjustments = Promise.resolve();
  let control: PreviewStreamControl | null = null;
  const dialogs = new BrowserDialog();
  let viewport: readonly [number, number] = [0, 0];
  let busy = false;
  let error: string | null = null;
  let listFailed = false;
  let sequence = 0;
  let painting: number | null = null;
  let waiting: ArrayBuffer | null = null;
  let revision = -1;

  function emit() {
    if (lifetime.aborted) return;
    publish({
      tabs: tabs.map(tabView),
      selected,
      dialog: dialogs.current,
      canWrite: !!service && session.browserWritable,
      canControl: control?.canOperate === true,
      controlled: control?.controller === "you",
      status: selected ? previewStreamControlLabel(control, m) : "",
      busy: busy || !service,
      viewport,
      error: error ?? (listFailed ? m(msg("Could not load browser tabs.")) : null),
    });
  }
  function frame(jpeg: ArrayBuffer) {
    if (lifetime.aborted || !selected) return;
    if (painting !== null) {
      waiting = jpeg;
      return;
    }
    painting = ++sequence;
    paint(selected, painting, jpeg);
  }
  function detach() {
    checking?.abort();
    checking = undefined;
    attaching?.abort();
    attaching = undefined;
    stream?.stop();
    stream = undefined;
    waiting = null;
    control = null;
    dialogs.update(null);
    viewport = [0, 0];
  }
  function streamAccess(signal: AbortSignal) {
    return Effect.runPromise(
      resolveDeviceHubAccess({
        prepared: preparedConnection(session),
        hubBasePath: PREVIEW_STREAM_BASE_PATH,
      }).pipe(Effect.provide(session.http)),
      { signal },
    );
  }
  async function select(id: string | null, refreshTicket = true) {
    detach();
    selected = id;
    error = null;
    emit();
    if (!id || lifetime.aborted) return;
    attaching = new AbortController();
    const lease = AbortSignal.any([attaching.signal, lifetime]);
    try {
      const access = await streamAccess(lease);
      if (lease.aborted) return;
      const target = {
        threadId,
        tabId: id,
        access,
        maxWidth: 1600,
        maxHeight: 1200,
        interactive: session.browserWritable,
      };
      stream = createPreviewStreamClient(target, {
        onFrame(jpeg) {
          if (!lease.aborted) frame(jpeg);
        },
        onViewport(size) {
          viewport = [size.width, size.height];
          emit();
        },
        onControl(next) {
          if (lease.aborted) return;
          dialogs.update(next);
          control = next;
          emit();
        },
        onConnectedChange(connected) {
          if (lease.aborted) return;
          if (!connected) {
            control = null;
            dialogs.update(null);
          }
          emit();
        },
        onUnauthorized() {
          if (lease.aborted) return;
          if (refreshTicket) void select(id, false);
          else {
            error = m(msg("Could not connect to the browser."));
            emit();
          }
        },
        onGone() {
          if (lease.aborted) return;
          error = m(msg("Could not connect to the browser."));
          emit();
          void refresh();
        },
        onHostSetup(setup) {
          error = m(msg("Browser setup required")) + ": " + setup.command;
          emit();
        },
      });
    } catch {
      if (!lease.aborted) {
        error = m(msg("Could not connect to the browser."));
        emit();
      }
    }
  }

  async function checkDialog(id: number, tabId: string) {
    if (checking) return;
    const check = new AbortController();
    checking = check;
    error = null;
    emit();
    const timeout = AbortSignal.timeout(10_000);
    const signal = AbortSignal.any([check.signal, lifetime, timeout]);
    let observer: PreviewStreamClient | undefined;
    signal.addEventListener(
      "abort",
      () => {
        observer?.stop();
        if (checking === check) checking = undefined;
        if (timeout.aborted && !lifetime.aborted) {
          error = m(msg("Could not connect to the browser."));
          emit();
        }
      },
      { once: true },
    );
    const failed = () => {
      if (signal.aborted) return;
      error = m(msg("Could not connect to the browser."));
      check.abort();
      emit();
    };
    try {
      const access = await streamAccess(signal);
      if (signal.aborted) return;
      observer = createPreviewStreamClient(
        { threadId, tabId, access, interactive: false, maxWidth: 1, maxHeight: 1 },
        {
          onControl(next) {
            if (signal.aborted) return;
            dialogs.rechecked(id, next.dialog);
            check.abort();
            emit();
          },
          onFrame() {},
          onViewport() {},
          onConnectedChange() {},
          onUnauthorized: failed,
          onGone: failed,
          onHostSetup: failed,
        },
      );
    } catch {
      failed();
    }
  }

  async function refresh(preferred?: string) {
    const rpc = service;
    if (!rpc || lifetime.aborted) return;
    const result = await Effect.runPromise(rpc[WS_METHODS.previewList]({ threadId }), {
      signal: lifetime,
    }).catch(() => null);
    if (lifetime.aborted || service !== rpc) return;
    if (!result) {
      listFailed = true;
      emit();
      return;
    }
    if (result.revision < revision) return;
    listFailed = false;
    revision = result.revision;
    const present = new Map(
      result.sessions
        .filter((tab) => tab.runtime === "server" || tab.headless)
        .map((tab) => [tab.tabId, tab] as const),
    );
    const retained: PreviewSessionSnapshot[] = [];
    for (const tab of tabs) {
      const current = present.get(tab.tabId);
      if (current) {
        retained.push(current);
        present.delete(tab.tabId);
      }
    }
    tabs = [...retained, ...present.values()];
    const choice =
      [preferred, selected].find((id) => tabs.some((tab) => tab.tabId === id)) ??
      tabs[0]?.tabId ??
      null;
    if (choice !== selected) void select(choice);
    else emit();
  }
  const connect = Effect.fn("nativeDesktop.browserFeed")(function* () {
    const rpc = yield* openEnvironmentRpc(session);
    error = null;
    service = rpc;
    revision = -1;
    yield* Effect.addFinalizer(() =>
      Effect.sync(() => {
        service = undefined;
        detach();
        selected = null;
        emit();
      }),
    );
    const events = rpc[WS_METHODS.subscribePreviewEvents]({}).pipe(
      Stream.filter((event) => event.threadId === threadId),
    );
    const feed = yield* events.pipe(
      Stream.runForEach(() => Effect.promise(() => refresh())),
      Effect.forkScoped,
    );
    yield* Effect.promise(() => refresh());
    yield* Fiber.join(feed);
    return yield* Effect.fail("Browser subscription closed");
  });
  let connecting = false;
  function start() {
    if (connecting || lifetime.aborted) return;
    connecting = true;
    void Effect.runPromise(connect().pipe(Effect.scoped, Effect.provide(session.http)), {
      signal: lifetime,
    })
      .catch(() => {
        error = m(msg("Could not load browser tabs."));
        emit();
      })
      .finally(() => {
        connecting = false;
      });
  }
  lifetime.addEventListener("abort", detach, { once: true });
  start();
  return {
    stop() {
      stopped.abort();
    },
    async request(request: BrowserRequest) {
      if (lifetime.aborted) return;
      if (request.action === "painted") {
        if (painting === request.sequence) {
          painting = null;
          const next = waiting;
          waiting = null;
          if (next) frame(next);
        }
        return;
      }
      if (request.action === "retry") {
        error = null;
        if (!service) start();
        else {
          if (selected) void select(selected);
          await refresh();
        }
        return;
      }
      if (!service) return;
      if (request.action === "select") {
        if (request.tab !== selected && tabs.some((t) => t.tabId === request.tab))
          void select(request.tab);
        return;
      }
      if (request.action === "recheckDialog") {
        const pending = dialogs.current;
        if (
          request.tab === selected &&
          pending?.id === request.dialogId &&
          pending.recheck &&
          control?.controller === "you"
        ) {
          await checkDialog(request.dialogId, request.tab);
        }
        return;
      }
      if (request.action === "dialog") {
        if (request.tab === selected && session.browserWritable) {
          dialogs.answer(
            request.dialogId,
            (dialog) =>
              stream?.send({
                type: "dialog",
                accept: request.accept,
                ...(dialog.type === "prompt" && request.accept
                  ? { promptText: request.promptText }
                  : {}),
              }) === true,
          );
          emit();
        }
        return;
      }
      if (request.action === "input") {
        if (request.tab === selected && session.browserWritable) stream?.send(request.input);
        return;
      }
      if (
        busy ||
        !session.browserWritable ||
        (request.action !== "add" && request.tab !== selected)
      )
        return;
      const rpc = service;
      if (request.action === "adjust") {
        const selection = attaching?.signal;
        if (!selection) return;
        const apply = async () => {
          if (selection.aborted || lifetime.aborted || busy || !session.browserWritable) return;
          const tab = tabs.find((t) => t.tabId === request.tab);
          const change = tab && tabAdjustment(tab, request.setting);
          if (!change) return;
          error = null;
          const before = revision;
          const updated = await Effect.runPromise(
            rpc[WS_METHODS.previewAdjust]({ threadId, tabId: request.tab, ...change }),
            { signal: lifetime },
          );
          if (selection.aborted || lifetime.aborted) return;
          if (before === revision) {
            tabs = tabs.map((t) => (t.tabId === updated.tabId ? updated : t));
            emit();
          }
          await refresh();
        };
        adjustments = adjustments.then(apply).catch(() => {
          if (!selection.aborted && !lifetime.aborted) {
            error = m(msg("Could not update the browser tab."));
            emit();
          }
        });
        return adjustments;
      }
      busy = true;
      error = null;
      emit();
      try {
        if (request.action === "close") {
          await Effect.runPromise(rpc[WS_METHODS.previewClose]({ threadId, tabId: request.tab }), {
            signal: lifetime,
          });
          await refresh();
        } else {
          const url = request.value.trim() ? address(request.value) : undefined;
          if (request.action === "navigate" && selected) {
            if (url && !stream?.send({ type: "navigate", url })) throw new Error();
          } else {
            const opened = await Effect.runPromise(
              rpc[WS_METHODS.previewOpen]({ threadId, runtime: "server", url }),
              { signal: lifetime },
            );
            await refresh(opened.tabId);
          }
        }
      } catch {
        error = m(msg("Could not update the browser tab."));
      } finally {
        busy = false;
        emit();
      }
    },
  };
}
