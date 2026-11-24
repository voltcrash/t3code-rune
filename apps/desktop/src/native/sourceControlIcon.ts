import { sourceControlClients } from "@t3tools/client-runtime/source-control-clients";

/** A host's mark as an icon body native can draw, from the same artwork the web draws. */
export function sourceControlIconBody(tracker: string) {
  const icon = sourceControlClients.find(tracker)?.icon;
  if (!icon) return null;
  const [x = 0, y = 0, width = 0, height = 0] = icon.viewBox.split(/\s+/u).map(Number);
  if (!(width > 0 && height > 0)) return null;
  // Rune draws a body in a square viewBox from the origin, so centre the mark in one.
  const view = Math.max(width, height);
  const shift = `translate(${(view - width) / 2 - x} ${(view - height) / 2 - y})`;
  const paint = (value: string | { readonly gradient: string } | undefined) =>
    value === undefined ? "none" : typeof value === "string" ? value : `url(#${value.gradient})`;
  const defs = (icon.gradients ?? [])
    .map(
      (g) =>
        `<linearGradient id="${g.id}" x1="${g.x1}" y1="${g.y1}" x2="${g.x2}" y2="${g.y2}" gradientUnits="userSpaceOnUse"${g.transform ? ` gradientTransform="${g.transform}"` : ""}>${g.stops
          .map(
            (stop) =>
              `<stop offset="${stop.offset}" stop-color="${stop.color}"${stop.opacity === undefined ? "" : ` stop-opacity="${stop.opacity}"`}/>`,
          )
          .join("")}</linearGradient>`,
    )
    .join("");
  const paths = icon.paths
    .map(
      (path) =>
        `<path d="${path.d}" fill="${paint(path.fill)}"${path.fillRule ? ` fill-rule="${path.fillRule}"` : ""}${path.fillOpacity === undefined ? "" : ` fill-opacity="${path.fillOpacity}"`}${path.stroke ? ` stroke="${path.stroke}"` : ""}${path.strokeWidth === undefined ? "" : ` stroke-width="${path.strokeWidth}"`}/>`,
    )
    .join("");
  return {
    name: `tracker-${tracker}`,
    view,
    body: `${defs ? `<defs>${defs}</defs>` : ""}<g transform="${shift}">${paths}</g>`,
  };
}
