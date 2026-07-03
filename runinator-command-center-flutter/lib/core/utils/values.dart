// port of core/utils/values.ts.

import 'dart:convert';

/// format an unknown value for UI display without relying on implicit object stringification.
String displayValue(Object? value) {
  if (value == null) {
    return '';
  }

  if (value is String) {
    return value;
  }

  if (value is num || value is bool) {
    return value.toString();
  }

  return jsonEncode(value);
}

/// shared emptiness check for required-field gates. a value counts as blank when
/// it is null, an empty list, or a string that is empty or only whitespace.
/// whitespace-only strings must not satisfy a required field.
bool isBlankValue(Object? value) {
  if (value == null) {
    return true;
  }

  if (value is String) {
    return value.trim().isEmpty;
  }

  if (value is List) {
    return value.isEmpty;
  }

  return false;
}
