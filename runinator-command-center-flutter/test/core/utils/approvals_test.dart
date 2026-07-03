import 'package:runinator_command_center_flutter/core/domain/models/workflow/node_run.dart';
import 'package:runinator_command_center_flutter/core/utils/approvals.dart';
import 'package:test/test.dart';

void main() {
  group('approval utils', () {
    test('treats workflow approval wait statuses consistently', () {
      expect(isApprovalWaitingStatus('approval-required'), isTrue);
      expect(isApprovalWaitingStatus('approval_required'), isTrue);
      expect(isApprovalWaitingStatus('pending'), isTrue);
      expect(isApprovalWaitingStatus('succeeded'), isFalse);
    });

    test('reads approval ids from workflow node run state', () {
      const nodeRun = WorkflowNodeRun(
        id: '00000000-0000-0000-0000-000000000001',
        workflowRunId: '00000000-0000-0000-0000-000000000010',
        nodeId: 'approval',
        status: 'approval_required',
        attempt: 1,
        parameters: {},
        state: {'approval_id': '00000000-0000-0000-0000-000000000042'},
        message: null,
      );

      expect(approvalIdFromNodeRun(nodeRun), '00000000-0000-0000-0000-000000000042');
    });

    test('selects the pending approval for a workflow node', () {
      final approval = selectWorkflowApprovalRecord(
        [
          {
            'id': '00000000-0000-0000-0000-000000000002',
            'workflow_run_id': '00000000-0000-0000-0000-000000000010',
            'node_id': 'approval',
            'status': 'approved',
          },
          {
            'id': '00000000-0000-0000-0000-000000000003',
            'workflow_run_id': '00000000-0000-0000-0000-000000000011',
            'node_id': 'approval',
            'status': 'pending',
          },
          {
            'id': '00000000-0000-0000-0000-000000000004',
            'workflow_run_id': '00000000-0000-0000-0000-000000000010',
            'node_id': 'approval',
            'status': 'pending',
          },
        ],
        '00000000-0000-0000-0000-000000000010',
        'approval',
      );

      expect(approval?['id'], '00000000-0000-0000-0000-000000000004');
    });
  });
}
