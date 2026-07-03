// port of core/utils/json.ts.
//
// named json_utils.dart (not json.dart) to avoid colliding with
// core/domain/json.dart, which ports a different source file (core/domain/json.ts).

import 'dart:convert';

import '../domain/json.dart';

JsonRecord parseObject(String text, JsonRecord fallback) {
  try {
    final value = jsonDecode(text.isEmpty ? '{}' : text);
    return isJsonObject(value) ? value as JsonRecord : fallback;
  } catch (_) {
    return fallback;
  }
}

JsonRecord? parseRequiredObject(String text) {
  try {
    final value = jsonDecode(text.isEmpty ? '{}' : text);

    if (isJsonObject(value)) {
      return value as JsonRecord;
    }
  } catch (_) {
    // surfaced by caller.
  }

  return null;
}

JsonValue parseRequiredJson(String text) {
  try {
    return jsonDecode(text.isEmpty ? 'null' : text);
  } catch (_) {
    // surfaced by caller.
  }

  return null;
}

T cloneJson<T>(T value) => jsonDecode(jsonEncode(value)) as T;

/// legacy alias used by a few call sites.
JsonRecord parseObjectRecord(String text, JsonRecord fallback) => parseObject(text, fallback);
