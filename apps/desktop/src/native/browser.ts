import {
  createPreviewStreamClient,
  PREVIEW_STREAM_BASE_PATH,
  previewStreamControlLabel,
  type PreviewStreamClient,
  type PreviewStreamControl,
  type PreviewStreamInput,
} from "@t3tools/client-runtime/preview/server-browser-stream";
import { resolveDeviceHubAccess } from "@t3tools/client-runtime/state/deviceHubAccess";
import { type PreviewSessionSnapshot, ThreadId, WS_METHODS } from "@t3tools/contracts";
import { resolveAddressBarInput } from "@t3tools/shared/preview";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as Stream from "effect/Stream";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";
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
  };
}

type Rpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type BrowserRequest =
  | { action: "add" | "navigate"; value: string; tab: string | null }
  | { action: "select"; tab: string }
  | { action: "close"; tab: string }
  | { action: "input"; tab: string; input: PreviewStreamInput }
  | { action: "retry" }
  | { action: "painted"; sequence: number };

function tabView(tab: PreviewSessionSnapshot) {
  const nav = tab.navStatus;
  return {
    id: tab.tabId,
    url: nav._tag === "Idle" ? "" : nav.url,
    title: nav._tag === "Idle" ? m(msg("New tab")) : nav.title || nav.url,
    back: tab.canGoBack,
    forward: tab.canGoForward,
    failure: nav._tag === "LoadFailed" ? nav.description : null,
  };
}

type Page = {
  tabs: ReturnType<typeof tabView>[];
  selected: string | null;
  canWrite: boolean;
  controlled: boolean;
  canControl: boolean;
  status: string;
  viewport: readonly [number, number];
  busy: boolean;
  error: string | null;
};

export function openBrowserPanel(
  session: Session,
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
  let control: PreviewStreamControl | null = null;
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
    attaching?.abort();
    attaching = undefined;
    stream?.stop();
    stream = undefined;
    waiting = null;
    control = null;
    viewport = [0, 0];
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
      const access = await Effect.runPromise(
        resolveDeviceHubAccess({
          prepared: preparedConnection(session),
          hubBasePath: PREVIEW_STREAM_BASE_PATH,
        }).pipe(Effect.provide(session.http)),
        { signal: lease },
      );
      if (lease.aborted) return;
      stream = createPreviewStreamClient(
        {
          threadId,
          tabId: id,
          access,
          maxWidth: 1600,
          maxHeight: 1200,
          interactive: session.browserWritable,
        },
        {
          onFrame(jpeg) {
            if (!lease.aborted) frame(jpeg);
          },
          onViewport(size) {
            viewport = [size.width, size.height];
            emit();
          },
          onControl(next) {
            control = next;
            emit();
          },
          onConnectedChange(connected) {
            if (!connected) control = null;
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
        },
      );
    } catch {
      if (!lease.aborted) {
        error = m(msg("Could not connect to the browser."));
        emit();
      }
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
    tabs = result.sessions.filter((tab) => tab.runtime === "server" || tab.headless);
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
