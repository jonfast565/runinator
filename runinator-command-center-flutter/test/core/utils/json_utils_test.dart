import 'package:runinator_command_center_flutter/core/utils/json_utils.dart';
import 'package:test/test.dart';

void main() {
  group('json utils', () {
    test('parses JSON objects', () {
      expect(parseRequiredObject('{"a":1}'), {'a': 1});
    });

    test('rejects arrays as required objects', () {
      expect(parseRequiredObject('[]'), isNull);
    });

    test('falls back for invalid JSON', () {
      expect(parseObject('{', {'fallback': true}), {'fallback': true});
    });

    test('clones JSON-compatible values', () {
      final source = <String, Object?>{'definition': <String, Object?>{'nodes': <Object?>[<String, Object?>{'id': 'start'}]}};
      final cloned = cloneJson<Map<String, Object?>>(source);
      expect(cloned, source);
      expect(identical(cloned, source), isFalse);
    });
  });
}
