import {
  EnvironmentId,
  USAGE_CONTRACT_VERSION,
  WS_METHODS,
  type UsageSummary,
  type UsageSummaryInput,
} from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import { expect, it, vi } from "vite-plus/test";
import { watchUsageTotals } from "./usageTotals.ts";

const owner = {
  environmentId: EnvironmentId.make("test"),
  label: "Test",
  canReadDiagnostics: true,
};
const settings = { usagePriceOverrides: {}, usageModelAliases: {} };

it("cancels the previous period read and publishes all hours of the newly selected window", async () => {
  const started = Promise.withResolvers<AbortSignal>();
  const completed = Promise.withResolvers<void>();
  const publish = vi.fn<Parameters<typeof watchUsageTotals>[2]>((view) => {
    if (!view.loading) completed.resolve();
  });
  const signal = new AbortController();
  const feed = watchUsageTotals(owner, signal.signal, publish);
  const summary = (input: UsageSummaryInput): UsageSummary => ({
    ...input,
    readAt: "2026-11-20T00:00:00Z",
    contractVersion: USAGE_CONTRACT_VERSION,
    scanDurationMs: 0,
    buckets: [],
    sources: [],
    pricing: { status: "unavailable", knownModels: 0, source: "fixture", fetchedAt: null },
  });
  const read = vi.fn((input: UsageSummaryInput) =>
    input.resolution === "hour"
      ? Effect.succeed(summary(input))
      : Effect.promise<UsageSummary>((abort) => {
          started.resolve(abort);
          return new Promise(() => {});
        }),
  );
  try {
    feed.configure(settings, {
      [WS_METHODS.serverGetUsageSummary]: read,
      [WS_METHODS.serverRefreshUsageRates]: () => Effect.never,
    });
    feed.select(1, 30);
    const previous = await started.promise;
    feed.select(2, 1);
    await completed.promise;
    expect(previous.aborted).toBe(true);
    expect(
      publish.mock.calls.filter(([reply]) => !reply.loading).map(([reply]) => reply.selection),
    ).toEqual([2]);
    expect(publish.mock.lastCall?.[0].data?.periods).toHaveLength(24);
    feed.close();
    feed.refresh();
    expect(read).toHaveBeenCalledTimes(2);
  } finally {
    signal.abort();
  }
});

it("shows restricted usage without reading diagnostics or refreshing rates", () => {
  const signal = new AbortController();
  const publish = vi.fn<Parameters<typeof watchUsageTotals>[2]>();
  const read = vi.fn(() => Effect.never);
  const feed = watchUsageTotals({ ...owner, canReadDiagnostics: false }, signal.signal, publish);
  try {
    feed.configure(settings, {
      [WS_METHODS.serverGetUsageSummary]: read,
      [WS_METHODS.serverRefreshUsageRates]: read,
    });
    feed.select(1, 7);
    feed.refresh();
    expect(publish.mock.lastCall?.[0]).toMatchObject({
      selection: 1,
      loading: false,
      restricted: true,
    });
    expect(read).not.toHaveBeenCalled();
  } finally {
    signal.abort();
  }
});
