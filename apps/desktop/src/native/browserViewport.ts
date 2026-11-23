import {
  resolveResponsiveBrowserViewportSize,
  resizeFreeformViewport,
} from "@t3tools/client-runtime/preview/viewport-layout";
import {
  PREVIEW_VIEWPORT_MAX_AREA,
  PREVIEW_VIEWPORT_MIN_DIMENSION,
  PREVIEW_VIEWPORT_MAX_DIMENSION,
  PreviewViewportSize,
  type PreviewViewportSetting,
} from "@t3tools/contracts";
import { PREVIEW_VIEWPORT_PRESETS, resolvePreviewViewport } from "@t3tools/shared/previewViewport";
import * as Schema from "effect/Schema";
import { msg } from "gt-i18n";
import { m } from "../i18n/catalog.ts";

const validSize = Schema.is(PreviewViewportSize);

export function browserDeviceLabels() {
  return {
    show: m(msg("Show device toolbar")),
    hide: m(msg("Hide device toolbar")),
    close: m(msg("Close device toolbar")),
    title: m(msg("Browser device toolbar")),
    dimensions: m(msg("Dimensions")),
    handles: [
      m(msg("Resize browser viewport from left edge")),
      m(msg("Resize browser viewport from right edge")),
      m(msg("Resize browser viewport from bottom edge")),
      m(msg("Resize browser viewport from bottom-left corner")),
      m(msg("Resize browser viewport from bottom-right corner")),
    ],
    limits: {
      min: PREVIEW_VIEWPORT_MIN_DIMENSION,
      max: PREVIEW_VIEWPORT_MAX_DIMENSION,
      area: PREVIEW_VIEWPORT_MAX_AREA,
    },
    preset: m(msg("Browser device preset")),
    responsive: m(
      msg("Responsive", { $context: "browser viewport preset that follows the panel size" }),
    ),
    width: m(msg("Viewport width")),
    height: m(msg("Viewport height")),
    rotate: m(msg("Rotate viewport")),
    lock: m(msg("Lock viewport aspect ratio")),
    unlock: m(msg("Unlock viewport aspect ratio")),
    presets: PREVIEW_VIEWPORT_PRESETS.map(({ id, label, detail }) => ({ id, label, detail })),
  };
}

export type DeviceChange =
  | { kind: "show"; width: number; height: number }
  | { kind: "fill" }
  | { kind: "responsive" }
  | { kind: "preset"; id: string }
  | {
      kind: "edit" | "rotate";
      edit?: {
        width: number;
        height: number;
        aspectRatio: number | null;
        axis: "width" | "height";
      };
    };

export function deviceViewport(
  change: DeviceChange,
  current: PreviewViewportSetting,
  initial: PreviewViewportSetting,
  zoom: number,
): PreviewViewportSetting | undefined {
  if (change.kind === "show") {
    if (current._tag !== "fill") return;
    if (initial._tag !== "fill") return initial;
    const size =
      change.width > 0 && change.height > 0
        ? resolveResponsiveBrowserViewportSize(change, zoom)
        : { width: 1024, height: 768 };
    return { _tag: "freeform", ...size };
  }
  if (current._tag === "fill") return;
  if (change.kind === "fill") return { _tag: "fill" };
  if (change.kind === "preset") {
    const preset = PREVIEW_VIEWPORT_PRESETS.find((p) => p.id === change.id);
    return preset && resolvePreviewViewport({ mode: "preset", preset: preset.id });
  }
  if (change.kind === "responsive")
    return { _tag: "freeform", width: current.width, height: current.height };
  const edit = change.edit;
  let next = current;
  if (
    edit &&
    [edit.width, edit.height].every(
      (n) =>
        Number.isInteger(n) &&
        n >= PREVIEW_VIEWPORT_MIN_DIMENSION &&
        n <= PREVIEW_VIEWPORT_MAX_DIMENSION,
    )
  ) {
    const size =
      edit.aspectRatio !== null
        ? resizeFreeformViewport(
            edit,
            { x: 0, y: 0 },
            1,
            edit.axis === "width" ? "east" : "south",
            edit.aspectRatio,
          )
        : { width: edit.width, height: edit.height };
    if (validSize(size) && (size.width !== current.width || size.height !== current.height)) {
      next = { _tag: "freeform", ...size };
    }
  }
  return change.kind === "rotate" ? { ...next, width: next.height, height: next.width } : next;
}
