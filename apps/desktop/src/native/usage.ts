import * as Fiber from "effect/Fiber";
import { getProviderClient } from "@t3tools/client-runtime/provider-clients";
import { refreshUsageLimits } from "@t3tools/client-runtime/state/usage";
import { WS_METHODS, type ServerConfig } from "@t3tools/contracts";
import {
  collectLimitAccounts,
  collectLimitPools,
  displayLimitWindows,
  remainingPercent,
  type LimitAccount,
} from "@t3tools/shared/usageLimits";
import {
  collectExternalUsageLinks,
  collectLimitNotices,
  formatResetsIn,
} from "@t3tools/shared/usageLimitsCopy";
import * as Clock from "effect/Clock";
import * as Effect from "effect/Effect";
import { msg } from "gt-i18n";
import { m, onLocaleChange } from "../i18n/catalog.ts";
import type { openEnvironmentRpc } from "./environmentRpc.ts";
import type { Session } from "./projects.ts";

type Rpc = Pick<
  Effect.Success<ReturnType<typeof openEnvironmentRpc>>,
  typeof WS_METHODS.serverRefreshProviders
>;
type LimitConfig = Pick<ServerConfig, "providers" | "usageLimitSources">;
type Environment = Pick<Session, "environmentId" | "label">;

export function usageLabels() {
  return {
    usage: m(msg("Usage")),
    limits: m(msg("Limits")),
    refresh: m(msg("Refresh")),
    close: m(msg("Close")),
    manage: m(msg("Manage usage")),
    reveal: m(msg("Click to reveal email")),
    hide: m(msg("Click to hide email")),
    loading: m(msg("Loading…")),
    empty: m(msg("No provider on the selected environments reports subscription limits.")),
  };
}

function accountIdentity(account: LimitAccount) {
  const emailInName = account.displayName?.includes("@") ? account.displayName : null;
  const name = emailInName ? null : account.displayName;
  return {
    name:
      name ??
      account.sourceLabel ??
      getProviderClient(account.driver)?.label ??
      String(account.driver),
    email: account.email ?? emailInName,
  };
}

function limitsView(owner: Environment, config: LimitConfig, now: number) {
  const presentations = new Map([
    [owner.environmentId, { entry: { target: { label: owner.label } }, serverConfig: config }],
  ]);
  const pools = collectLimitPools(collectLimitAccounts(presentations), now);
  return {
    groups: pools.map((pool) => ({
      name: getProviderClient(pool.driver)?.label ?? String(pool.driver),
      windows: displayLimitWindows(pool).map((window) => ({
        name: window.label,
        remaining: window.remainingPercent,
        accounts: window.members.map(({ account, window: member }) => ({
          ...accountIdentity(account),
          remaining: remainingPercent(member),
          reset: formatResetsIn(member, now, m),
        })),
      })),
    })),
    notices: collectLimitNotices(presentations, m),
    links: collectExternalUsageLinks(presentations, m).filter(({ url }) => {
      const protocol = URL.parse(url)?.protocol;
      return protocol === "http:" || protocol === "https:";
    }),
  };
}

export function watchUsage(
  owner: Environment,
  signal: AbortSignal,
  publish: (value: {
    labels: ReturnType<typeof usageLabels>;
    result: ReturnType<typeof limitsView> | null;
    refreshing: boolean;
    error: string | null;
  }) => void,
) {
  let config: LimitConfig | undefined;
  let rpc: Rpc | undefined;
  let open = false;
  let pending: AbortController | undefined;
  let error: string | null = null;
  let result: ReturnType<typeof limitsView> | null = null;
  let countdown: Fiber.Fiber<void> | undefined;
  const clearCountdown = () => {
    if (countdown) Effect.runFork(Fiber.interrupt(countdown));
    countdown = undefined;
  };
  const emit = () => {
    if (open && !signal.aborted)
      publish({ labels: usageLabels(), result, refreshing: pending !== undefined, error });
  };
  const update = () => {
    if (!open || signal.aborted) return;
    result = config ? limitsView(owner, config, Effect.runSync(Clock.currentTimeMillis)) : null;
    emit();
    clearCountdown();
    if (
      result?.groups.some((group) =>
        group.windows.some((window) => window.accounts.some((a) => a.reset)),
      )
    ) {
      countdown = Effect.runFork(
        Effect.sleep("1 minute").pipe(
          Effect.map(() => {
            countdown = undefined;
            update();
          }),
        ),
      );
    }
  };
  const refresh = async (automatic = false) => {
    if (!open || !rpc || !config || pending || signal.aborted) return;
    const connected = rpc;
    const request = new AbortController();
    pending = request;
    error = null;
    emit();
    try {
      await refreshUsageLimits(
        owner.environmentId,
        () =>
          Effect.runPromise(
            connected[WS_METHODS.serverRefreshProviders]({
              workspaceSnapshotsOnDemand: true,
              omitProviders: true,
            }).pipe(Effect.timeout("60 seconds")),
            { signal: AbortSignal.any([signal, request.signal]) },
          ),
        automatic,
      );
    } catch {
      if (!request.signal.aborted) error = m(msg("Couldn’t refresh provider status. Try again."));
    } finally {
      if (pending === request) {
        pending = undefined;
        update();
      }
    }
  };
  const cancel = () => {
    clearCountdown();
    pending?.abort();
    pending = undefined;
    error = null;
  };
  const releaseLocale = onLocaleChange(update);
  signal.addEventListener(
    "abort",
    () => {
      cancel();
      releaseLocale();
    },
    { once: true },
  );
  return {
    configure(next: LimitConfig | undefined, connection: Rpc | undefined) {
      const reconnect = rpc !== connection;
      if (reconnect) cancel();
      const changed =
        reconnect ||
        config?.providers !== next?.providers ||
        config?.usageLimitSources !== next?.usageLimitSources;
      config = next;
      rpc = connection;
      if (changed) update();
      if (reconnect) void refresh(true);
    },
    select(visible: boolean) {
      if (open === visible) return;
      open = visible;
      cancel();
      result = null;
      update();
      if (visible) void refresh(true);
    },
    refresh,
    async manage(url: string, opener: (url: string) => Promise<boolean>) {
      if (!open || signal.aborted || !result?.links.some((link) => link.url === url)) return;
      if (!(await opener(url)) && open && !signal.aborted) {
        error = m(msg("Couldn’t open the link."));
        emit();
      }
    },
  };
}
