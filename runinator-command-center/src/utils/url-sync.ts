// pure mapping between the URL hash and (tab, id) route state, split out from useUrlSync so it can
// be unit-tested without a DOM. the hash layout is #/<Tab> or #/<Tab>/<id>.

export interface ParsedRoute {
  tab: string | null;
  id: string | null;
}

export function parseRoute(hash: string, isKnownTab: (tab: string) => boolean): ParsedRoute {
  const raw = hash.replace(/^#\/?/, "");

  if (!raw) {
    return { tab: null, id: null };
  }

  const [tabPart, idPart] = raw.split("/");
  const tab = isKnownTab(tabPart) ? tabPart : null;
  return { tab, id: idPart ? decodeURIComponent(idPart) : null };
}

export function formatRoute(tab: string, id: string | null): string {
  return id ? `#/${tab}/${encodeURIComponent(id)}` : `#/${tab}`;
}
