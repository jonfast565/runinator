import 'package:runinator_command_center_flutter/core/utils/status.dart';
import 'package:test/test.dart';

void main() {
  group('status utils', () {
    test('maps terminal failures', () {
      expect(statusBadgeClass('failed'), 'status-failed');
      expect(statusClassForNode('timed_out'), 'node-danger');
    });

    test('maps active statuses', () {
      expect(statusBadgeClass('running'), 'status-running');
      expect(statusBadgeClass('queued'), 'status-waiting');
      expect(statusBadgeClass('debug_paused'), 'status-waiting');
      expect(statusClassForNode('waiting'), 'node-waiting');
      expect(statusClassForNode('approval_required'), 'node-waiting');
      expect(statusClassForNode('debug_paused'), 'node-warning');
    });

    test('identifies terminal workflow run statuses', () {
      expect(isTerminalWorkflowRunStatus('succeeded'), isTrue);
      expect(isTerminalWorkflowRunStatus('failed'), isTrue);
      expect(isTerminalWorkflowRunStatus('timed_out'), isTrue);
      expect(isTerminalWorkflowRunStatus('canceled'), isTrue);
      expect(isTerminalWorkflowRunStatus('blocked'), isFalse);
      expect(isTerminalWorkflowRunStatus('running'), isFalse);
    });
  });
}
