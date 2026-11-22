import { getProviderClient } from "@t3tools/client-runtime/provider-clients";
import {
  normalizeProviderAccentColor,
  providerAccentForegroundColor,
  providerInstanceInitials,
  shouldShowInstanceBadge,
} from "@t3tools/client-runtime/state/provider-instance-display";
import {
  brandProviderInstanceEntries,
  deriveProviderInstanceEntries,
} from "@t3tools/client-runtime/state/provider-instances";
import {
  officialAcpRegistryIconUrlForAgentId,
  resolveOfficialAcpRegistryIconUrl,
  type ServerConfig,
} from "@t3tools/contracts";
import * as NodeUtil from "node:util";

type Mark = NonNullable<NonNullable<ReturnType<typeof getProviderClient>>["icon"]>;

function svg(viewBox: string, body: string) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${body}</svg>`;
}

function packageSvg(icon: Mark, theme: "light" | "dark") {
  const paths = icon.paths.map((path, index) => {
    const gradient = path.gradient;
    let fill = (path.fill ?? icon.fill)[theme];
    let paint = "";
    if (gradient) {
      const stops = gradient.stops
        .map((stop) => `<stop offset="${stop.offset}" stop-color="${stop.color}"/>`)
        .join("");
      paint = `<defs><linearGradient id="g${index}" x1="${gradient.x1}" y1="${gradient.y1}" x2="${gradient.x2}" y2="${gradient.y2}" gradientUnits="userSpaceOnUse">${stops}</linearGradient></defs>`;
      fill = `url(#g${index})`;
    }
    return `${paint}<path d="${path.d}" fill="${fill}" fill-rule="${path.fillRule ?? "nonzero"}"/>`;
  });
  return svg(icon.viewBox, paths.join(""));
}

function driverArt(driver: string) {
  const packaged = getProviderClient(driver)?.icon;
  if (packaged) return { light: packageSvg(packaged, "light"), dark: packageSvg(packaged, "dark") };
  return null;
}

function instanceRows(config: ServerConfig) {
  const entries = brandProviderInstanceEntries(
    deriveProviderInstanceEntries(config.providers),
    config.settings,
  );
  return entries.map((entry) => {
    const accent = normalizeProviderAccentColor(entry.accentColor);
    return {
      id: entry.instanceId,
      driver: entry.driverKind,
      initials: providerInstanceInitials(entry.displayName),
      registry:
        entry.driverKind === "acpRegistry"
          ? (resolveOfficialAcpRegistryIconUrl(entry.acpRegistryIconUrl) ??
            officialAcpRegistryIconUrlForAgentId(entry.acpRegistryAgentId))
          : null,
      badge: entry.driverKind !== "openaiCompatible" && shouldShowInstanceBadge(entry, entries),
      accent,
      ink: accent ? providerAccentForegroundColor(accent) : undefined,
    };
  });
}

export function watchSidebarProviders(
  publish: (
    instances: ReturnType<typeof instanceRows>,
    art: Record<string, NonNullable<ReturnType<typeof driverArt>>>,
  ) => void,
) {
  let previous: ReturnType<typeof instanceRows> | undefined;
  const sent = new Set<string>();
  return (config: ServerConfig | undefined) => {
    if (!config) return;
    const instances = instanceRows(config);
    if (NodeUtil.isDeepStrictEqual(previous, instances)) return;
    previous = instances;
    const art: Record<string, NonNullable<ReturnType<typeof driverArt>>> = {};
    for (const { driver } of instances) {
      if (sent.has(driver)) continue;
      const mark = driverArt(driver);
      if (mark) art[driver] = mark;
      sent.add(driver);
    }
    publish(instances, art);
  };
}
