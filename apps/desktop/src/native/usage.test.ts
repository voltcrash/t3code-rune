import {
  EnvironmentId,
  ProviderDriverKind,
  UsageLimitSourceId,
  WS_METHODS,
  type UsageLimitSourceSnapshot,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { expect, it, vi } from "vite-plus/test";
import { watchUsage } from "./usage.ts";

it("publishes the connected environment’s limit sources only while Usage is open", () => {
  const owner = { environmentId: EnvironmentId.make("usage-test"), label: "Test environment" };
  const checkedAt = "2026-01-01T00:00:00.000Z";
  const source: UsageLimitSourceSnapshot = {
    id: UsageLimitSourceId.make("test"),
    label: "Test hub",
    kind: "cliproxy",
    checkedAt,
    accounts: [
      {
        id: "fixture",
        driver: ProviderDriverKind.make("codex"),
        email: "account@example.test",
        usageLimits: {
          checkedAt,
          windows: [{ id: "weekly", kind: "weekly", label: "Weekly", usedPercent: 23 }],
        },
      },
    ],
  };
  const signal = new AbortController();
  const publish = vi.fn<Parameters<typeof watchUsage>[2]>();
  const feed = watchUsage(owner, signal.signal, publish);
  const rpc = { [WS_METHODS.serverRefreshProviders]: () => Effect.never };
  try {
    feed.configure({ providers: [], usageLimitSources: [source] }, rpc);
    expect(publish).not.toHaveBeenCalled();
    feed.select(true);
    expect(publish.mock.lastCall?.[0].result?.groups[0]?.windows[0]).toMatchObject({
      remaining: 77,
      accounts: [{ email: "account@example.test", remaining: 77 }],
    });
    feed.configure(undefined, undefined);
    expect(publish.mock.lastCall?.[0].result).toBeNull();
    feed.configure({ providers: [], usageLimitSources: [] }, rpc);
    expect(publish.mock.lastCall?.[0].result?.groups).toEqual([]);
    feed.select(false);
    publish.mockClear();
    feed.configure({ providers: [], usageLimitSources: [source] }, rpc);
    expect(publish).not.toHaveBeenCalled();
  } finally {
    signal.abort();
  }
});
