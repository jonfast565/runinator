import 'package:runinator_command_center_flutter/core/utils/secrets.dart';
import 'package:test/test.dart';

void main() {
  group('secret utils', () {
    test('round trips secret references', () {
      const value = 'secret://github/token%2Fmain';
      expect(secretRef('github', 'token/main'), value);
      expect(parseSecretRef(value)?.scope, 'github');
      expect(parseSecretRef(value)?.name, 'token/main');
      expect(secretRefLabel(value), 'github/token/main');
    });

    test('ignores non-secret values', () {
      expect(parseSecretRef('plain text'), isNull);
      expect(secretRefLabel('plain text'), '');
    });
  });
}
