import 'package:runinator_command_center_flutter/core/utils/resources.dart';
import 'package:test/test.dart';

void main() {
  group('resource utils', () {
    test('derives jira summaries from key and title', () {
      expect(
        genericRecordSummary({'provider': 'jira', 'key': 'ABC-1', 'title': 'Fix it'}),
        'ABC-1 Fix it',
      );
    });

    test('uses explicit type fields first', () {
      expect(genericRecordType({'approval_type': 'manual'}, 'approvals'), 'manual');
    });

    test('falls back to endpoint type names', () {
      expect(genericRecordType({}, 'external_items'), 'external_item');
      expect(genericRecordType({}, 'automation_events'), 'event');
    });
  });
}
