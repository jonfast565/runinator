// port of core/utils/approvals.ts.

import '../domain/models/index.dart';
import 'values.dart';

enum ApprovalAction { approve, reject }

bool isApprovalWaitingStatus(Object? status) =>
    ['waiting', 'approval_required', 'pending'].contains(_normalizeStatus(status));

String? approvalIdFromNodeRun(WorkflowNodeRun nodeRun) {
  final fromState = nonEmptyString(nodeRun.state?['approval_id']);
  if (fromState != null) {
    return fromState;
  }

  final outputJson = nodeRun.outputJson;
  final fromOutput = outputJson is Map<String, Object?> ? nonEmptyString(outputJson['approval_id']) : null;
  if (fromOutput != null) {
    return fromOutput;
  }

  final approval = nodeRun.state?['approval'];
  final fromApproval = approval is Map<String, Object?> ? nonEmptyString(approval['id']) : null;
  return fromApproval;
}

Map<String, Object?>? selectWorkflowApprovalRecord(
  List<Map<String, Object?>> records,
  String workflowRunId,
  String nodeId,
) {
  final matches = records
      .where((record) =>
          nonEmptyString(record['id']) != null &&
          displayValue(record['workflow_run_id']) == workflowRunId &&
          displayValue(record['node_id']) == nodeId)
      .toList()
    ..sort((left, right) {
      final rankDiff = _approvalRecordRank(left) - _approvalRecordRank(right);
      if (rankDiff != 0) {
        return rankDiff;
      }
      return _recordTime(right).compareTo(_recordTime(left));
    });

  return matches.isNotEmpty ? matches.first : null;
}

String? nonEmptyString(Object? value) {
  final text = value is String ? value.trim() : '';
  return text.isNotEmpty ? text : null;
}

int _approvalRecordRank(Map<String, Object?> record) => isApprovalWaitingStatus(record['status']) ? 0 : 1;

int _recordTime(Map<String, Object?> record) {
  final raw = record['updated_at'] ?? record['created_at'];
  if (raw is! String) {
    return 0;
  }

  final parsed = DateTime.tryParse(raw);
  return parsed?.millisecondsSinceEpoch ?? 0;
}

String _normalizeStatus(Object? status) => displayValue(status).trim().toLowerCase().replaceAll('-', '_');
