// `URL.parse` and `URL.canParse` are missing before Safari 17/18, and client code ships unpolyfilled.
export function parseUrl(value: string, base?: string): URL | null {
  try {
    return new URL(value, base);
  } catch {
    return null;
  }
}

export function resolveExternalWebLink(href: string | null | undefined): URL | null {
  if (!href) return null;
  const url = parseUrl(href.startsWith("//") ? `https:${href}` : href);
  return url?.protocol === "http:" || url?.protocol === "https:" ? url : null;
}
