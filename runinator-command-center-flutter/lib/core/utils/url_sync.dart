// port of core/utils/url-sync.ts.
//
// pure mapping between a URL hash and (tab, id) route state, kept dependency-free
// so it can be unit-tested without a browser. the hash layout is #/<Tab> or
// #/<Tab>/<id>.

class ParsedRoute {
  const ParsedRoute({required this.tab, required this.id});

  final String? tab;
  final String? id;
}

ParsedRoute parseRoute(String hash, bool Function(String tab) isKnownTab) {
  final raw = hash.replaceFirst(RegExp(r'^#/?'), '');

  if (raw.isEmpty) {
    return const ParsedRoute(tab: null, id: null);
  }

  final parts = raw.split('/');
  final tabPart = parts[0];
  final idPart = parts.length > 1 ? parts[1] : null;
  final tab = isKnownTab(tabPart) ? tabPart : null;

  return ParsedRoute(
    tab: tab,
    id: (idPart != null && idPart.isNotEmpty) ? Uri.decodeComponent(idPart) : null,
  );
}

String formatRoute(String tab, String? id) =>
    id != null ? '#/$tab/${Uri.encodeComponent(id)}' : '#/$tab';
