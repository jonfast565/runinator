// port of core/utils/status.ts.

bool isBadStatus(String? status) =>
    status != null && ['blocked', 'failed', 'rejected', 'timed_out', 'canceled'].contains(status);

bool isGoodStatus(String? status) =>
    status != null && ['approved', 'succeeded', 'passed', 'open'].contains(status);

bool isTerminalWorkflowRunStatus(String? status) =>
    ['succeeded', 'failed', 'timed_out', 'canceled'].contains(status ?? '');

String statusBadgeClass(String? status) {
  if (isBadStatus(status)) {
    return 'status-failed';
  }

  if (isGoodStatus(status)) {
    return 'status-succeeded';
  }

  if (status == 'running') {
    return 'status-running';
  }

  if ([
    'queued',
    'waiting',
    'approval_required',
    'input_required',
    'debug_paused',
    'paused',
    'pending',
  ].contains(status ?? '')) {
    return 'status-waiting';
  }

  return 'status-muted';
}

String statusClassForNode(String? status) {
  if (['succeeded', 'passed', 'approved'].contains(status ?? '')) {
    return 'node-success';
  }

  if (['failed', 'rejected', 'timed_out', 'canceled', 'blocked'].contains(status ?? '')) {
    return 'node-danger';
  }

  if (status == 'running') {
    return 'node-running';
  }

  if ([
    'waiting',
    'approval_required',
    'input_required',
    'approval-required',
    'pending',
  ].contains(status ?? '')) {
    return 'node-waiting';
  }

  if (['debug_paused', 'paused', 'queued'].contains(status ?? '')) {
    return 'node-warning';
  }

  if (status != null && status.isNotEmpty) {
    return 'node-active';
  }

  return '';
}
