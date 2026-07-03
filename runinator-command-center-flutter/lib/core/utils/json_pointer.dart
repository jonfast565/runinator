// port of core/utils/json-pointer.ts.

class PointerResult {
  const PointerResult({required this.value, required this.exists, this.error});

  final Object? value;
  final bool exists;
  final String? error;
}

/// resolve a JSON pointer (RFC 6901) or dotted/bracketed path against a value.
/// accepts "/foo/bar", "foo.bar", "foo[0].bar". returns exists: false when
/// any segment is missing; never throws.
PointerResult evaluatePointer(Object? input, String pointer) {
  final trimmed = pointer.trim();

  if (trimmed.isEmpty) {
    return PointerResult(value: input, exists: true);
  }

  final segments = _parseSegments(trimmed);
  Object? current = input;

  for (final seg in segments) {
    if (current == null) {
      return const PointerResult(value: null, exists: false);
    }

    if (current is List) {
      final index = int.tryParse(seg);
      if (index == null || index < 0 || index >= current.length) {
        return const PointerResult(value: null, exists: false);
      }
      current = current[index];
      continue;
    }

    if (current is! Map) {
      return const PointerResult(value: null, exists: false);
    }

    if (!current.containsKey(seg)) {
      return const PointerResult(value: null, exists: false);
    }

    current = current[seg];
  }

  return PointerResult(value: current, exists: true);
}

List<String> _parseSegments(String pointer) {
  if (pointer.startsWith('/')) {
    return pointer.substring(1).split('/').map((s) => s.replaceAll('~1', '/').replaceAll('~0', '~')).toList();
  }

  final segments = <String>[];
  var buf = '';

  var i = 0;
  while (i < pointer.length) {
    final ch = pointer[i];

    if (ch == '.') {
      if (buf.isNotEmpty) {
        segments.add(buf);
      }
      buf = '';
    } else if (ch == '[') {
      if (buf.isNotEmpty) {
        segments.add(buf);
      }
      buf = '';
      final end = pointer.indexOf(']', i);

      if (end < 0) {
        break;
      }

      final inner = pointer.substring(i + 1, end);
      segments.add(inner.replaceFirst(RegExp(r"""^['"]"""), '').replaceFirst(RegExp(r"""['"]$"""), ''));
      i = end;
    } else {
      buf += ch;
    }

    i++;
  }

  if (buf.isNotEmpty) {
    segments.add(buf);
  }

  return segments;
}
