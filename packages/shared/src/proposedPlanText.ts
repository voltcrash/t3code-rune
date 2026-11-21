export function proposedPlanTitle(planMarkdown: string): string | null {
  const heading = planMarkdown.match(/^\s{0,3}#{1,6}\s+(.+)$/m)?.[1]?.trim();
  return heading && heading.length > 0 ? heading : null;
}

export function proposedPlanBodyStart(planMarkdown: string): number {
  const markdown = planMarkdown.trimEnd();
  let at = 0;
  let end = markdown.indexOf("\n");
  const line = () => markdown.slice(at, end < 0 ? markdown.length : end).replace(/\r$/, "");
  const advance = () => {
    at = end < 0 ? markdown.length : end + 1;
    end = markdown.indexOf("\n", at);
  };
  const skipBlank = () => {
    while (at < markdown.length && !line().trim()) advance();
  };
  if (/^\s{0,3}#{1,6}\s+/.test(line())) advance();
  skipBlank();
  if (
    line()
      .match(/^\s{0,3}#{1,6}\s+(.+)$/)?.[1]
      ?.trim()
      .toLowerCase() === "summary"
  ) {
    advance();
    skipBlank();
  }
  return at;
}

export function stripDisplayedPlanMarkdown(planMarkdown: string): string {
  return planMarkdown.slice(proposedPlanBodyStart(planMarkdown)).trimEnd().replaceAll("\r\n", "\n");
}

export function proposedPlanCanCollapse(markdown: string): boolean {
  return markdown.length > 900 || markdown.split("\n").length > 20;
}

export function proposedPlanPreviewEnd(markdown: string, maxLines: number): number | undefined {
  let visible = 0;
  for (const line of markdown.matchAll(/[^\n]+/g)) {
    if (line[0].trim().length && ++visible > maxLines) return line.index;
  }
  return undefined;
}
