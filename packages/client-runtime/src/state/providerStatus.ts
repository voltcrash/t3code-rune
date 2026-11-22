import type { ServerProvider } from "@t3tools/contracts";
import { msg, mFallback } from "gt-i18n";
import { formatProviderDriverKindLabel } from "./providerInstanceDisplay.ts";

function getIncompatibleVersion(status: ServerProvider) {
  const compatibility = status.compatibilityAdvisory;
  if (status.status === "error" && status.auth.status === "unauthenticated") return null;
  return compatibility?.status === "broken" ||
    (status.status === "ready" && compatibility?.status === "unsupported")
    ? compatibility
    : null;
}

export function getProviderStatusBannerKey(status: ServerProvider | null): string | null {
  if (!status || status.status === "disabled") return null;
  const incompatible = getIncompatibleVersion(status);
  if (incompatible) {
    return [
      status.instanceId,
      incompatible.status,
      status.version ?? "",
      incompatible.message ?? "",
    ].join("\u0000");
  }
  if (status.status === "ready") return null;
  // Antigravity validates credentials at session start; unknown restart auth is not failure.
  if (
    status.driver === "antigravity" &&
    status.installed &&
    status.status === "warning" &&
    status.auth.status === "unknown"
  ) {
    return null;
  }
  return [status.instanceId, status.status, status.auth.status, status.message ?? ""].join(
    "\u0000",
  );
}

export function shouldShowProviderStatusBanner(
  status: ServerProvider | null,
  isDismissed: boolean,
): boolean {
  return getProviderStatusBannerKey(status) !== null && !isDismissed;
}

// Built-in instance ids such as `codex` repeat across environments.
export function getProviderStatusDismissalKey(
  environmentId: string | undefined,
  status: ServerProvider | null,
): string | null {
  const bannerKey = getProviderStatusBannerKey(status);
  return bannerKey === null ? null : `${environmentId ?? ""}\u0000${bannerKey}`;
}

export function hasProviderSetup(status: ServerProvider): boolean {
  return (
    status.driver === "antigravity" ||
    status.setup?.canAuthenticate === true ||
    status.setup?.canInstall === true
  );
}

export function getProviderStatusMessage(
  status: ServerProvider,
  resolve: (descriptor: string) => string = mFallback,
): string {
  if (
    status.auth.status !== "unauthenticated" &&
    status.compatibilityAdvisory?.status === "broken" &&
    status.compatibilityAdvisory.message
  ) {
    return status.compatibilityAdvisory.message;
  }
  if (status.message) return status.message;
  const providerName = status.displayName?.trim() || formatProviderDriverKindLabel(status.driver);
  if (!status.installed && hasProviderSetup(status)) {
    return resolve(
      msg("Open provider setup to install {provider} on this environment.", {
        provider: formatProviderDriverKindLabel(status.driver),
      }),
    );
  }
  if (status.auth.status === "unauthenticated") {
    if (hasProviderSetup(status)) {
      return status.driver === "antigravity"
        ? resolve(msg("Open provider setup to sign in with Google."))
        : resolve(msg("Open provider setup to sign in."));
    }
    return resolve(msg("Sign in via the CLI to authenticate again."));
  }
  return status.status === "ready"
    ? resolve(msg("No models are available for this provider."))
    : status.status === "error"
      ? resolve(msg("{provider} provider is unavailable.", { provider: providerName }))
      : resolve(msg("{provider} provider has limited availability.", { provider: providerName }));
}

export function providerStatusPresentation(
  status: ServerProvider,
  resolve: (descriptor: string) => string = mFallback,
) {
  const provider = status.displayName?.trim() || formatProviderDriverKindLabel(status.driver);
  const incompatible = getIncompatibleVersion(status);
  const version = status.version ?? "";
  const title =
    status.status === "error" && status.auth.status === "unauthenticated"
      ? msg("{provider} is unauthenticated", { provider })
      : incompatible?.status === "broken"
        ? msg("{provider} {version} is known to be broken", { provider, version })
        : incompatible
          ? msg("{provider} {version} is unsupported", { provider, version })
          : msg("{provider} provider status", { provider });
  return {
    providerName: provider,
    title: resolve(title),
    message: incompatible?.message ?? getProviderStatusMessage(status, resolve),
    isWarning:
      incompatible?.status !== "broken" && (status.status === "warning" || incompatible !== null),
    isAdvisory: incompatible !== null && incompatible.status !== "broken",
  };
}
