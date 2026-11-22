// @effect-diagnostics nodeBuiltinImport:off -- Tag the loading reply synchronously so a replaced query cannot publish with another query’s settings.
import * as NodeCrypto from "node:crypto";
import * as NodeUtil from "node:util";
import { getProviderClient } from "@t3tools/client-runtime/provider-clients";
import { usageSummaryInput } from "@t3tools/client-runtime/state/usage";
import {
  EnvironmentAuthorizationError,
  USAGE_CONTRACT_VERSION,
  WS_METHODS,
  type ServerConfig,
  type UsageSummary,
  type UsageSummaryInput,
  type UsageProviderKind,
} from "@t3tools/contracts";
import { stableStringify } from "@t3tools/shared/relaySigning";
import { niceScale, smoothCurve } from "@t3tools/shared/usageChart";
import {
  enumerateDays,
  enumerateHourStarts,
  formatCount,
  formatDayShort,
  formatHourShort,
  formatPercent,
  formatTokens,
  formatUsd,
  makeWindow,
} from "@t3tools/shared/usageFormat";
import { formatUsageContractMismatch, usageSourceMessages } from "@t3tools/shared/usageFormatCopy";
import { isModelCostUnknown, mergeUsage } from "@t3tools/shared/usageMerge";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import { msg } from "gt-i18n";
import { currentLanguage, m, onLocaleChange } from "../i18n/catalog.ts";
import type { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

const isAuthorizationError = Schema.is(EnvironmentAuthorizationError);

type EnvironmentRpc = Effect.Success<ReturnType<typeof openEnvironmentRpc>>;
type Rpc = Pick<EnvironmentRpc, typeof WS_METHODS.serverRefreshUsageRates> & {
  [WS_METHODS.serverGetUsageSummary]: (
    input: UsageSummaryInput,
  ) => Effect.Effect<
    UsageSummary,
    Effect.Error<ReturnType<EnvironmentRpc[typeof WS_METHODS.serverGetUsageSummary]>>
  >;
};
type Environment = Pick<Session, "environmentId" | "label" | "canReadDiagnostics">;
type Settings = Pick<
  ServerConfig["settings"],
  "usageModelFilter" | "usageModelAliases" | "usagePriceOverrides"
>;
type Period = 1 | 7 | 30 | 90;

type Snapshot = { summary: UsageSummary; window: UsageSummaryInput };
function providerLabel(kind: UsageProviderKind) {
  if (kind === "claude") return "Claude Code";
  if (kind === "grok") return "Grok Build";
  return getProviderClient(kind)?.label ?? kind;
}

function totals(owner: Environment, { summary, window }: Snapshot) {
  const merged = mergeUsage(
    [{ environmentId: owner.environmentId, label: owner.label, summary }],
    USAGE_CONTRACT_VERSION,
  );
  const mismatch = merged.contractMismatches[0];
  if (mismatch) return { error: formatUsageContractMismatch(owner.label, mismatch, m) };
  const locale = currentLanguage();
  const metric = (cost: number, tokens: number) => ({
    cost: formatUsd(cost, locale),
    tokens: formatTokens(tokens),
  });
  const hourly = window.sinceTime !== undefined && window.untilTime !== undefined;
  const periods = hourly
    ? enumerateHourStarts(window.sinceTime, window.untilTime)
    : enumerateDays(window.sinceDay, window.untilDay);
  const buckets = hourly
    ? new Map(merged.hourly.map((bucket) => [bucket.hourStart, bucket]))
    : new Map(merged.daily.map((bucket) => [bucket.day, bucket]));
  const hour12 = new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions().hour12;
  const providers = merged.providers.filter((p) => p.totalTokens > 0 || p.costUsd > 0);
  const chart = (mode: "cost" | "tokens") => {
    const field = mode === "cost" ? "costUsd" : "totalTokens";
    const lines = providers.map(({ provider }) => ({
      provider,
      values: periods.map((period) => buckets.get(period)?.byProvider.get(provider)?.[field] ?? 0),
    }));
    const { max, ticks } = niceScale(Math.max(0, ...lines.flatMap((line) => line.values)), 4);
    const y = (value: number) => (max ? 260 - (value / max) * 252 : 260);
    const x = 960 / Math.max(1, periods.length - 1);
    return {
      ticks: ticks.map((tick) => ({
        y: y(tick),
        label: tick === 0 ? "0" : mode === "cost" ? formatUsd(tick, locale) : formatTokens(tick),
      })),
      lines: lines.map(({ provider, values }) => ({
        provider,
        start: y(values[0] ?? 0),
        curve: smoothCurve(values.map((v, i) => ({ x: i * x, y: y(v) }))).map(({ c1, c2, to }) => [
          c1.y,
          c2.y,
          to.y,
        ]),
      })),
    };
  };
  return {
    data: {
      total: metric(merged.costUsd, merged.totalTokens),
      sessions: m(
        msg("{count, plural, one {{formatted} sessions} other {{formatted} sessions}}", {
          count: merged.sessions,
          formatted: formatCount(merged.sessions, locale),
        }),
      ),
      categories: [
        {
          label: m(msg("Input")),
          cost: merged.categoryCost.input,
          tokens: merged.uncachedInputTokens,
          ink: 0.6,
        },
        {
          label: m(msg("Cache read")),
          cost: merged.categoryCost.cacheRead,
          tokens: merged.cachedInputTokens,
          ink: 0.3,
        },
        {
          label: m(msg("Cache write")),
          cost: merged.categoryCost.cacheWrite,
          tokens: merged.cacheCreationTokens,
          ink: 0.72,
        },
        {
          label: m(msg("Output")),
          cost: merged.categoryCost.output,
          tokens: merged.outputTokens,
          ink: 1,
        },
        {
          label: m(msg("Other")),
          cost: merged.categoryCost.unsplit >= 0.005 ? merged.categoryCost.unsplit : 0,
          tokens: 0,
          ink: 0.44,
        },
      ]
        .filter((c) => c.cost > 0 || c.tokens > 0)
        .map((c) => ({ ...c, display: metric(c.cost, c.tokens) })),
      savings: formatUsd(merged.costQuality.cacheSavingsUsd, locale),
      charts: { cost: chart("cost"), tokens: chart("tokens") },
      cached: formatTokens(merged.cachedInputTokens),
      input: formatTokens(merged.uncachedInputTokens),
      output: formatTokens(merged.outputTokens),
      unpriced: merged.costQuality.unpricedShare > 0,
      providers: providers.map((p) => ({
        driver: p.provider,
        sessions: m(
          msg("{count, plural, one {{formatted} session} other {{formatted} sessions}}", {
            count: p.sessions,
            formatted: formatCount(p.sessions, locale),
          }),
        ),
        costShare: m(
          msg("{share} of cost · {tokens} tokens", {
            share: formatPercent(p.costShare),
            tokens: formatTokens(p.totalTokens),
          }),
        ),
        tokenShare: m(
          msg("{share} of tokens · {cost}", {
            share: formatPercent(p.tokenShare),
            cost: formatUsd(p.costUsd, locale),
          }),
        ),
        name: providerLabel(p.provider),
        ...metric(p.costUsd, p.totalTokens),
      })),
      periods: periods.map((period) => {
        const bucket = buckets.get(period);
        const cost = bucket?.costUsd ?? 0;
        const tokens = bucket?.totalTokens ?? 0;
        return {
          name: hourly
            ? formatHourShort(period, window.timeZone, hour12, locale)
            : formatDayShort(period, locale),
          cost,
          tokens,
          display: metric(cost, tokens),
        };
      }),
      models: merged.models.map((model) => ({
        name: model.model,
        provider: model.provider,
        costShare: isModelCostUnknown(model) ? "" : formatPercent(model.costShare),
        tokenShare: formatPercent(model.tokenShare),
        cost: isModelCostUnknown(model) ? m(msg("Unpriced")) : formatUsd(model.costUsd, locale),
        tokens: formatTokens(model.totalTokens),
        count: model.totalTokens,
        price: model.costUsd,
      })),
      notices: usageSourceMessages(summary.sources, m),
    },
  };
}

export function watchUsageTotals(
  owner: Environment,
  lifetime: AbortSignal,
  publish: (reply: {
    selection: number;
    scope?: string;
    data?: NonNullable<ReturnType<typeof totals>["data"]>;
    error?: string;
    loading: boolean;
    restricted?: boolean;
    retryable?: boolean;
  }) => void,
) {
  let rpc: Rpc | undefined;
  let settings: Settings | undefined;
  let request: AbortController | undefined;
  let selected: { id: number; days: Period } | undefined;
  let last: Snapshot | undefined;
  let scope: string | undefined;
  const emit = (reply: Parameters<typeof publish>[0]) =>
    publish(scope ? { ...reply, scope } : reply);
  const stop = () => {
    request?.abort();
    request = undefined;
  };
  const restrict = (selection: number) => {
    last = undefined;
    emit({
      selection,
      loading: false,
      restricted: true,
      error: m(msg("This connection does not have access to diagnostics and usage.")),
    });
  };
  const read = async (refresh: boolean) => {
    stop();
    if (!selected || lifetime.aborted) return;
    const selection = selected.id;
    if (!owner.canReadDiagnostics) {
      restrict(selection);
      return;
    }
    const connected = rpc;
    if (!connected || !settings) {
      scope = undefined;
      emit({ selection, loading: true });
      return;
    }
    const job = new AbortController();
    request = job;
    const fresh = () => request === job && !lifetime.aborted;
    const signal = AbortSignal.any([lifetime, job.signal]);
    const window = makeWindow(
      selected.days,
      DateTime.toDateUtc(Effect.runSync(DateTime.now)),
      selected.days === 1 ? "hour" : "day",
    );
    const input = usageSummaryInput(window, settings.usageModelFilter);
    const nextScope = NodeCrypto.createHash("sha256")
      .update(
        stableStringify([
          selected.days,
          window.timeZone,
          input.modelFilter,
          settings.usagePriceOverrides,
          settings.usageModelAliases,
        ]),
      )
      .digest("hex");
    if (nextScope !== scope) last = undefined;
    scope = nextScope;
    emit({ selection, loading: true, ...(last && totals(owner, last)) });
    try {
      if (refresh)
        await Effect.runPromise(
          connected[WS_METHODS.serverRefreshUsageRates]({}).pipe(
            Effect.timeout("30 seconds"),
            Effect.ignore,
          ),
          { signal },
        );
      const get = (policy: { provisional: true } | { awaitRefresh: true }) =>
        Effect.runPromise(connected[WS_METHODS.serverGetUsageSummary]({ ...input, ...policy }), {
          signal,
        });
      let value: UsageSummary | undefined;
      if (!refresh) {
        try {
          value = await get({ provisional: true });
        } catch (error) {
          if (!fresh()) return;
          if (isAuthorizationError(error)) throw error;
        }
      }
      if (!fresh()) return;
      if (!value || value.sources.some((source) => source.refreshing)) {
        if (value) {
          last = { summary: value, window };
          emit({ selection, loading: true, ...totals(owner, last) });
        }
        value = await get({ awaitRefresh: true });
      }
      if (fresh()) {
        last = { summary: value, window };
        emit({ selection, loading: false, ...totals(owner, last) });
      }
    } catch (error) {
      if (!fresh()) return;
      if (isAuthorizationError(error)) {
        restrict(selection);
        return;
      }
      emit({
        selection,
        loading: false,
        retryable: !last,
        ...(last && totals(owner, last)),
        error: m(msg("Couldn’t load usage. Try again.")),
      });
    } finally {
      if (request === job) request = undefined;
    }
  };
  const releaseLocale = onLocaleChange(() => {
    if (selected && last)
      emit({ selection: selected.id, loading: request !== undefined, ...totals(owner, last) });
  });
  lifetime.addEventListener(
    "abort",
    () => {
      stop();
      releaseLocale();
    },
    { once: true },
  );
  return {
    configure(next: Settings | undefined, connected: Rpc | undefined) {
      const changed =
        rpc !== connected ||
        !NodeUtil.isDeepStrictEqual(settings?.usageModelFilter, next?.usageModelFilter) ||
        !NodeUtil.isDeepStrictEqual(settings?.usagePriceOverrides, next?.usagePriceOverrides) ||
        !NodeUtil.isDeepStrictEqual(settings?.usageModelAliases, next?.usageModelAliases);
      if (changed) last = undefined;
      settings = next;
      rpc = connected;
      if (changed) void read(false);
    },
    select(id: number, days: Period) {
      if (selected?.id === id && selected.days === days) return;
      selected = { id, days };
      last = undefined;
      void read(false);
    },
    close() {
      selected = undefined;
      last = undefined;
      stop();
    },
    refresh() {
      void read(true);
    },
  };
}
