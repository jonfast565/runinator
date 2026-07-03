// port of core/utils/format.ts.

import 'dart:convert';

String formatDate(String? value) {
  if (value == null || value.isEmpty) {
    return '-';
  }

  final date = DateTime.tryParse(value);
  return date == null ? value : date.toLocal().toString();
}

const JsonEncoder _prettyEncoder = JsonEncoder.withIndent('  ');

String pretty(Object? value) => _prettyEncoder.convert(value ?? <String, Object?>{});

/// extract a human-readable message from an unknown thrown value.
String errorMessage(Object? err) {
  if (err is Error) {
    return err.toString();
  }

  if (err is Exception) {
    return err.toString();
  }

  if (err is String) {
    return err;
  }

  if (err is Map && err.containsKey('message')) {
    final message = err['message'];
    return message is String ? message : message.toString();
  }

  return err.toString();
}

/// normalize a run/node error for display: unwrap common json envelopes and trim noise.
String formatErrorMessage(Object? raw) {
  if (raw == null) {
    return '';
  }

  var text = raw is String ? raw : jsonEncode(raw);
  text = text.trim();

  if (text.isEmpty) {
    return '';
  }

  final looksJson =
      (text.startsWith('{') && text.endsWith('}')) || (text.startsWith('[') && text.endsWith(']'));

  if (looksJson) {
    try {
      final parsed = jsonDecode(text);
      final extracted = _extractErrorText(parsed);
      return extracted.isNotEmpty ? extracted.trim() : _prettyEncoder.convert(parsed);
    } catch (_) {
      // not valid json after all; fall back to the raw text.
    }
  }

  return text;
}

/// pull a human message out of an error envelope like {"error":"...","message":"..."}.
String _extractErrorText(Object? value) {
  if (value is String) {
    return value;
  }

  if (value is Map) {
    for (final key in ['message', 'error', 'detail', 'reason', 'description']) {
      final candidate = value[key];

      if (candidate is String && candidate.trim().isNotEmpty) {
        return candidate;
      }

      if (candidate is Map) {
        final nested = _extractErrorText(candidate);

        if (nested.isNotEmpty) {
          return nested;
        }
      }
    }
  }

  return '';
}
