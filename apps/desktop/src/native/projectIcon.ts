import { deriveProjectIdentity } from "@t3tools/client-runtime/project-identity";
import type { Project, ProjectIconColor } from "@t3tools/contracts";
import { icons, type IconNode } from "lucide";
import colors from "tailwindcss/colors";

function projectColor(name: ProjectIconColor) {
  const components = (value: string) => value.slice("oklch(".length, -1).split(" ").map(parseFloat);
  return { light: components(colors[name][600]), dark: components(colors[name][400]) };
}

function lucideNodes(name: string): IconNode {
  const key = name
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join("");
  const catalog: Readonly<Record<string, IconNode>> = icons;
  for (const candidate of [key, key.replace(/^Lucide/, ""), key.replace(/Icon$/, "")]) {
    if (Object.hasOwn(catalog, candidate)) return catalog[candidate]!;
  }
  return icons.FolderCode;
}

export function projectGlyph(title: string, icon: Project["projectIcon"]) {
  if (icon?.kind === "lucide") {
    const body = lucideNodes(icon.name)
      .map(([tag, attributes]) => {
        const attrs = Object.entries(attributes)
          .map(([key, value]) => `${key}="${value}"`)
          .join(" ");
        return `<${tag} ${attrs}/>`;
      })
      .join("");
    return { kind: icon.kind, body, color: projectColor(icon.color) };
  }
  if (icon?.kind === "emoji") return icon;
  if (icon) return { ...icon, color: projectColor(icon.color) };
  const { monogram, color } = deriveProjectIdentity(title);
  return { kind: "monogram" as const, text: monogram, color: projectColor(color) };
}
