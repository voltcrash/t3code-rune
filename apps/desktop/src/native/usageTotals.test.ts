import {
  EnvironmentAuthorizationError,
  EnvironmentId,
  UsageReadError,
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
    buckets: [
      {
        day: input.untilDay,
        provider: "claude",
        model: "example",
        totals: {
          uncachedInputTokens: 42,
          cachedInputTokens: 0,
          cacheCreationTokens: 0,
          outputTokens: 0,
          reasoningTokens: 0,
        },
        costUsd: 1,
        cacheSavingsUsd: 0,
        costSource: "modelPriced",
        records: 1,
        unpricedRecords: 0,
        sessions: 1,
      },
    ],
    sources: [
      {
        fingerprint: {
          hostId: "fixture",
          provider: "claude",
          resolvedHomePath: "/fixture",
          volumeId: "1",
        },
        status: "ok",
        scannedFiles: 1,
        skippedFiles: 0,
        malformedRecords: 0,
        distinctSessions: 1,
        message: null,
      },
    ],
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
    expect(publish.mock.lastCall?.[0].data?.total.tokens).toBe("42");
    feed.filter(new Set(["claude"]));
    expect(publish.mock.lastCall?.[0].data?.total.tokens).toBe("0");
    feed.filter(new Set());
    expect(publish.mock.lastCall?.[0].data?.total.tokens).toBe("42");
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

it("distinguishes retryable read failures from a server denying diagnostics after reconnect", async () => {
  const lifetime = new AbortController();
  type Reply = Parameters<Parameters<typeof watchUsageTotals>[2]>[0];
  let result = Promise.withResolvers<Reply>();
  const feed = watchUsageTotals(owner, lifetime.signal, (reply) => {
    if (!reply.loading) result.resolve(reply);
  });
  const connect = (
    error: UsageReadError | EnvironmentAuthorizationError,
    config: NonNullable<Parameters<typeof feed.configure>[0]> = settings,
  ) =>
    feed.configure(config, {
      [WS_METHODS.serverGetUsageSummary]: () => Effect.fail(error),
      [WS_METHODS.serverRefreshUsageRates]: () => Effect.never,
    });
  try {
    feed.select(1, 7);
    const failure = new UsageReadError({ reason: "scanFailed", detail: "Temporary failure" });
    connect(failure);
    const original = await result.promise;
    expect(original).toMatchObject({ selection: 1, retryable: true });
    expect(original.scope).toBeTruthy();
    feed.configure(undefined, undefined);
    result = Promise.withResolvers<Reply>();
    connect(failure);
    expect((await result.promise).scope).toBe(original.scope);
    result = Promise.withResolvers<Reply>();
    connect(failure, { ...settings, usageModelFilter: { exclude: [{ model: "example" }] } });
    expect((await result.promise).scope).not.toBe(original.scope);
    feed.configure(undefined, undefined);
    result = Promise.withResolvers<Reply>();
    connect(
      new EnvironmentAuthorizationError({ message: "Denied", requiredScope: "diagnostics:read" }),
    );
    const denied = await result.promise;
    expect(denied.restricted).toBe(true);
    expect(denied.retryable).toBeUndefined();
  } finally {
    lifetime.abort();
  }
});
