import {
  type EnvironmentId,
  type ProviderInstanceId,
  type ServerProvider,
} from "@t3tools/contracts";
import { useAtomValue } from "@effect/atom-react";
import { T, useGT, useMessages } from "gt-react";
import { memo, useRef, useState, useSyncExternalStore } from "react";
import { InfoIcon, XIcon } from "lucide-react";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "../ui/alert";
import { Button, InlineButton } from "../ui/button";
import { m, type MessageResolver } from "../../i18n/messages";
import {
  getProviderStatusBannerKey,
  hasProviderSetup,
  providerStatusPresentation,
  getProviderStatusMessage as providerStatusMessage,
} from "@t3tools/client-runtime/state/provider-status";
import { Tooltip, TooltipPopup, TooltipTrigger } from "../ui/tooltip";
import { serverEnvironment } from "../../state/server";
import { useAtomCommand } from "../../state/use-atom-command";
import { RefreshIcon } from "../ui/refresh-icon";

// A dismissal lasts the session, not one view, so opening another thread cannot bring it back.
const sessionDismissedProviderStatusBannerKeys = new Set<string>();
const dismissalListeners = new Set<() => void>();

export function dismissProviderStatusBannerForSession(bannerKey: string | null): void {
  if (bannerKey === null || sessionDismissedProviderStatusBannerKeys.has(bannerKey)) return;
  sessionDismissedProviderStatusBannerKeys.add(bannerKey);
  for (const listener of dismissalListeners) listener();
}

export function isProviderStatusBannerDismissedForSession(bannerKey: string | null): boolean {
  return bannerKey !== null && sessionDismissedProviderStatusBannerKeys.has(bannerKey);
}

function subscribeToDismissals(listener: () => void) {
  dismissalListeners.add(listener);
  return () => dismissalListeners.delete(listener);
}

export function useProviderStatusBannerDismissed(bannerKey: string | null): boolean {
  return useSyncExternalStore(subscribeToDismissals, () =>
    isProviderStatusBannerDismissedForSession(bannerKey),
  );
}

export {
  getProviderStatusBannerKey,
  getProviderStatusDismissalKey,
  hasProviderSetup,
  shouldShowProviderStatusBanner,
} from "@t3tools/client-runtime/state/provider-status";

export function getProviderStatusMessage(status: ServerProvider, resolve: MessageResolver = m) {
  return providerStatusMessage(status, resolve);
}

export const ProviderStatusBanner = memo(function ProviderStatusBanner({
  environmentId,
  onDismiss,
  onOpenProviderSetup,
  status,
}: {
  environmentId: EnvironmentId;
  onDismiss: () => void;
  onOpenProviderSetup?: (instanceId: ProviderInstanceId) => void;
  status: ServerProvider | null;
}) {
  const gt = useGT();
  const messages = useMessages();
  const canRefresh = useAtomValue(serverEnvironment.refreshProviders.permissionAtom(environmentId));
  const refreshProviders = useAtomCommand(serverEnvironment.refreshProviders, {
    label: "Refresh provider status",
  });
  const refreshingRef = useRef(false);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const refresh = async () => {
    if (!status || !canRefresh || refreshingRef.current) return;
    refreshingRef.current = true;
    setIsRefreshing(true);
    await refreshProviders({
      environmentId,
      input: { instanceId: status.instanceId, refreshModels: true },
    });
    refreshingRef.current = false;
    setIsRefreshing(false);
  };

  if (!status || getProviderStatusBannerKey(status) === null) {
    return null;
  }

  const { providerName, title, message, isWarning, isAdvisory } = providerStatusPresentation(
    status,
    messages,
  );

  return (
    <div className="pointer-events-auto mx-auto w-fit max-w-[calc(100%-2rem)] pt-3">
      <Alert
        variant={isWarning ? "warning" : "error"}
        role={isAdvisory ? "status" : "alert"}
        surface="glass"
        controlAlignment="first-line"
      >
        <InfoIcon />
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>
          <div className="flex flex-col gap-2">
            <Tooltip>
              <TooltipTrigger render={<div className="line-clamp-3" />}>{message}</TooltipTrigger>
              <TooltipPopup side="top" className="whitespace-pre-wrap">
                {message}
              </TooltipPopup>
            </Tooltip>
            <div className="flex flex-wrap items-center justify-end gap-3">
              {onOpenProviderSetup && hasProviderSetup(status) ? (
                <InlineButton onClick={() => onOpenProviderSetup(status.instanceId)}>
                  <T>Open provider setup</T>
                </InlineButton>
              ) : null}
              <Button
                aria-label={gt("Refresh {provider} provider status", { provider: providerName })}
                disabled={!canRefresh || isRefreshing}
                onClick={() => void refresh()}
                size="xs"
                variant="outline"
              >
                <RefreshIcon />
                {isRefreshing ? gt("Refreshing...") : gt("Refresh")}
              </Button>
            </div>
          </div>
        </AlertDescription>
        <AlertAction>
          <Button
            aria-label={
              status.status === "error"
                ? gt("Dismiss {provider} provider error", { provider: providerName })
                : status.status === "warning"
                  ? gt("Dismiss {provider} provider warning", { provider: providerName })
                  : status.status === "ready"
                    ? gt("Dismiss {provider} provider ready", { provider: providerName })
                    : gt("Dismiss {provider} provider disabled", { provider: providerName })
            }
            onClick={onDismiss}
            size="icon-xs"
            variant="ghost-muted"
          >
            <XIcon />
          </Button>
        </AlertAction>
      </Alert>
    </div>
  );
});
