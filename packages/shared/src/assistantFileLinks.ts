import type { Root, RootContent } from "mdast";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import { unified } from "unified";
import { codexFileCitationMarkdown } from "./codexFileCitations.ts";
import {
  renderCodexInlineDirectivesAsMarkdown,
  remarkCodexDirectives,
} from "./codexMarkdownDirectives.ts";
import { fileLinkLabel } from "./fileLinks.ts";
import { markdownFileParentSuffixes } from "./markdownFileLabels.ts";
import {
  classifyMarkdownImageSource,
  inlineCodeFilePathCandidate,
  isMarkdownFileLinkLabel,
  resolveMarkdownFileLink,
  shouldRenderMarkdownImageAsFileLink,
} from "./markdownLinks.ts";
import { remarkKeepWindowsPaths } from "./markdownWindowsPaths.ts";
import { mediaKindFromPath } from "./mediaPreview.ts";

const parser = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkKeepWindowsPaths)
  .use(remarkCodexDirectives)
  .freeze();
type Node = Root | RootContent;
export type AssistantFileLink = { href: string; copy: string; prefix?: string };

function plainLabel(node: Node): string {
  if ("value" in node) return node.value;
  if ("alt" in node) return node.alt ?? "";
  return "children" in node ? node.children.map(plainLabel).join("") : "";
}

function fileHref(node: Node, definitions: ReadonlyMap<string, string>, cwd?: string) {
  switch (node.type) {
    case "link":
      return node.url;
    case "linkReference":
      return definitions.get(node.identifier);
    case "inlineCode":
      return inlineCodeFilePathCandidate(node.value);
    case "image":
    case "imageReference": {
      const url = node.type === "image" ? node.url : definitions.get(node.identifier);
      return url &&
        shouldRenderMarkdownImageAsFileLink(
          classifyMarkdownImageSource(url, cwd),
          mediaKindFromPath(url),
        )
        ? url
        : undefined;
    }
    default:
      return undefined;
  }
}

function aliasMarkdown(node: Node, source: string) {
  if (node.type === "text") {
    // A label moves into paragraph context, where its punctuation can acquire block or link syntax.
    return node.value.replace(/\r\n?|\n/g, " ").replace(/[\\`*_{}[\]()#+.!<>|&~=-]/g, "\\$&");
  }
  return source.slice(node.position?.start.offset, node.position?.end.offset);
}

export function assistantFileLinks(
  markdown: string,
  cwd: string | undefined,
): {
  body: string;
  links?: Record<string, AssistantFileLink>;
} {
  if (!/[[`]/.test(markdown) && !markdown.includes(":codex-file-citation"))
    return { body: markdown };
  const tree = parser.runSync(parser.parse(markdown), { value: markdown }) as Root;
  const definitions = new Map<string, string>();
  const destinations = new Set<string>();
  function index(node: Node) {
    if ("url" in node) destinations.add(node.url);
    if (node.type === "definition" && !definitions.has(node.identifier))
      definitions.set(node.identifier, node.url);
    if ("children" in node) node.children.forEach(index);
  }
  index(tree);
  const links: Record<string, AssistantFileLink> = {};
  const parts: string[] = [];
  let cursor = 0;
  let suffixes: ReturnType<typeof markdownFileParentSuffixes> | undefined;
  function visit(node: Node) {
    const start = node.position?.start.offset;
    const end = node.position?.end.offset;
    if (start !== undefined && end !== undefined) {
      if (node.data && "codexFollowUp" in node.data && "children" in node && node.children.length) {
        const from = node.children[0]?.position?.start.offset;
        const to = node.children.at(-1)?.position?.end.offset;
        if (from !== undefined && to !== undefined) {
          parts.push(markdown.slice(cursor, start));
          cursor = from;
          node.children.forEach(visit);
          parts.push(markdown.slice(cursor, to));
          cursor = end;
          return;
        }
      }
      const isLink = node.type === "link" || node.type === "linkReference";
      const isImage = node.type === "image" || node.type === "imageReference";
      const href = fileHref(node, definitions, cwd);
      const file = href && resolveMarkdownFileLink(href, cwd);
      if (href && file) {
        suffixes ??= markdownFileParentSuffixes(
          markdown,
          renderCodexInlineDirectivesAsMarkdown(markdown),
          cwd,
        );
        const label = plainLabel(node);
        const prefix = isLink && !isMarkdownFileLinkLabel(label, href) ? label + " " : undefined;
        const source = markdown.slice(start, end);
        const copy =
          isLink && !(source.startsWith("[") && source.includes("]("))
            ? `[${label.replace(/[\\[\]]/g, "\\$&")}](${href})`
            : source;
        let id = "rune-file:" + start;
        while (destinations.has(id)) id += ":";
        links[id] = { href, copy, ...(prefix ? { prefix } : {}) };
        parts.push(markdown.slice(cursor, start));
        if (prefix && "children" in node) {
          parts.push(node.children.map((child) => aliasMarkdown(child, markdown)).join("") + " ");
        }
        parts.push(
          codexFileCitationMarkdown({
            path: href,
            label: fileLinkLabel(file.position, suffixes),
            href: id,
          }),
        );
        cursor = end;
        return;
      }
      if (isLink || isImage) return;
    }
    if ("children" in node) node.children.forEach(visit);
  }
  visit(tree);
  if (!parts.length) return { body: markdown };
  parts.push(markdown.slice(cursor));
  return { body: parts.join(""), ...(Object.keys(links).length ? { links } : {}) };
}
