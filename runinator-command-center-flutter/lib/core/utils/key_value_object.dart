// port of core/utils/key-value-object.ts.

import '../domain/json.dart';

class RenameObjectKeyResult {
  const RenameObjectKeyResult({required this.value, required this.error});

  final JsonRecord value;
  final String error;
}

String uniqueObjectKey(JsonRecord record, [String base = 'key']) {
  var index = 1;
  var key = base;

  while (record.containsKey(key)) {
    index += 1;
    key = '${base}_$index';
  }

  return key;
}

RenameObjectKeyResult renameObjectKey(JsonRecord record, String previousKey, String nextKey) {
  final trimmed = nextKey.trim();

  if (trimmed.isEmpty) {
    return RenameObjectKeyResult(value: record, error: 'Key is required');
  }

  if (trimmed != previousKey && record.containsKey(trimmed)) {
    return RenameObjectKeyResult(value: record, error: 'Key already exists');
  }

  if (trimmed == previousKey) {
    return RenameObjectKeyResult(value: record, error: '');
  }

  final next = <String, Object?>{
    for (final entry in record.entries) (entry.key == previousKey ? trimmed : entry.key): entry.value,
  };

  return RenameObjectKeyResult(value: next, error: '');
}

JsonRecord setObjectValue(JsonRecord record, String key, Object? value) {
  if (key.trim().isEmpty) {
    return record;
  }

  return {...record, key: asJsonValue(value)};
}

JsonRecord removeObjectKey(JsonRecord record, String key) => {
      for (final entry in record.entries)
        if (entry.key != key) entry.key: entry.value,
    };
