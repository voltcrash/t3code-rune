import { unified } from "unified";
import remarkParse from "remark-parse";
import { remarkKeepWindowsPaths } from "./markdownWindowsPaths.ts";
import { isWindowsAbsolutePath } from "./path.ts";
import { buildFileLinkParentSuffixByPath } from "./fileLinks.ts";
import {
  extractMarkdownLinkHrefs,
  extractInlineCodeSpans,
  inlineCodeFilePathCandidate,
  resolveMarkdownFileLink,
} from "./markdownLinks.ts";

const linkDestinationProcessor = unified().use(remarkParse).use(remarkKeepWindowsPaths);

// An angle destination cannot hold an unescaped `<` or `>`, or end on a lone backslash.
const ANGLE_DESTINATION_CONTENT = /^(?:[^\\<>]|\\.)*$/;

/** Reads a destination from `extractMarkdownLinkHrefs` as the renderer's parser does. */
export function decodeMarkdownLinkEscapes(href: string): string {
  // Only the parser knows character references, so destinations that may have one go through it.
  if (href.includes("&")) {
    // The extractor drops angle brackets, and only that form keeps an unmatched `)` in the path.
    const destination = ANGLE_DESTINATION_CONTENT.test(href) ? `<${href}>` : href;
    const paragraph = linkDestinationProcessor.parse(`[](${destination})`).children[0];
    const link = paragraph?.type === "paragraph" ? paragraph.children[0] : undefined;
    if (link?.type === "link") return link.url;
  }
  return isWindowsAbsolutePath(href) ? href : href.replace(/\\([!-/:-@[-`{-~])/g, "$1");
}

export function markdownFileParentSuffixes(
  markdown: string,
  rendered: string,
  cwd: string | undefined,
) {
  const paths = [
    ...extractMarkdownLinkHrefs(rendered).map(decodeMarkdownLinkEscapes),
    ...extractInlineCodeSpans(markdown).flatMap((span) => inlineCodeFilePathCandidate(span) ?? []),
  ].flatMap((href) => {
    const resolved = resolveMarkdownFileLink(href, cwd);
    return resolved ? [resolved.position.path] : [];
  });
  return buildFileLinkParentSuffixByPath(paths);
}
