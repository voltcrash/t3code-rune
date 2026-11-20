import { useAtomValue } from "@effect/atom-react";
import type { EnvironmentIdentificationMode } from "@t3tools/contracts";
import { useId } from "react";
import blueprint from "../../../../assets/dev/sidebar.svg?raw";
import nightSky from "../../../../assets/nightly/sidebar.svg?raw";

import { APP_STAGE_LABEL } from "../branding";
import { resolveServerBackedAppStageLabel } from "../branding.logic";
import { primaryServerConfigAtom } from "../state/server";

export type SidebarStageBackdropVariant = "nightly" | "dev";
export type EnvironmentIdentificationPillLabel = "Dev" | "Nightly";

const STAGE_BACKDROP_VIEW_BOX = "0 0 8192 96";

export function resolveSidebarStageBackdropVariant(
  stageLabel: string,
  mode: EnvironmentIdentificationMode = "artwork",
): SidebarStageBackdropVariant | null {
  if (mode === "nightly-artwork") return "nightly";
  if (mode === "dev-artwork") return "dev";
  if (mode !== "artwork") return null;
  const normalized = stageLabel.trim().toLowerCase();
  if (normalized === "nightly") return "nightly";
  if (normalized === "dev") return "dev";
  return null;
}

export function resolveEnvironmentIdentificationPillLabel(
  stageLabel: string,
): EnvironmentIdentificationPillLabel | null {
  const normalized = stageLabel.trim().toLowerCase();
  // oxlint-disable-next-line t3code/no-untranslated-copy -- release channel names
  if (normalized === "dev") return "Dev";
  // oxlint-disable-next-line t3code/no-untranslated-copy -- release channel names
  if (normalized === "nightly") return "Nightly";
  return null;
}

export function useEnvironmentStageLabel(): string {
  const primaryServerVersion =
    useAtomValue(primaryServerConfigAtom)?.environment.serverVersion ?? null;

  return resolveServerBackedAppStageLabel({
    primaryServerVersion,
    fallbackStageLabel: APP_STAGE_LABEL,
  });
}

export function useSidebarStageBackdropVariant(
  mode: EnvironmentIdentificationMode,
): SidebarStageBackdropVariant | null {
  return resolveSidebarStageBackdropVariant(useEnvironmentStageLabel(), mode);
}

export function SidebarStageBackdrop({ variant }: { variant: SidebarStageBackdropVariant }) {
  return (
    <div
      aria-hidden
      className="sidebar-stage-backdrop pointer-events-none absolute inset-x-0 top-0 z-0 h-20 select-none overflow-hidden"
    >
      <StageBackdropArt variant={variant} />
    </div>
  );
}

const svgContents = (svg: string) => svg.slice(svg.indexOf(">") + 1, svg.lastIndexOf("</svg>"));
const artwork = { dev: svgContents(blueprint), nightly: svgContents(nightSky) };

function StageBackdropArt({
  variant,
  compact = false,
}: {
  variant: SidebarStageBackdropVariant;
  compact?: boolean;
}) {
  const id = useId().replaceAll(":", "");
  const markup = {
    __html: artwork[variant]
      .replaceAll('id="stage-', `id="${id}-stage-`)
      .replaceAll("url(#stage-", `url(#${id}-stage-`),
  };
  const viewBox = compact
    ? variant === "dev"
      ? "64 0 8192 96"
      : "96 0 8192 96"
    : STAGE_BACKDROP_VIEW_BOX;
  return (
    <svg
      data-stage-art={variant === "dev" ? "blueprint" : "nightly"}
      className="h-full w-full"
      fill="none"
      preserveAspectRatio="xMinYMin slice"
      viewBox={viewBox}
      xmlns="http://www.w3.org/2000/svg"
      dangerouslySetInnerHTML={markup}
    />
  );
}

export function StageBackdropButtonArt({ variant }: { variant: SidebarStageBackdropVariant }) {
  return <StageBackdropArt variant={variant} compact />;
}
