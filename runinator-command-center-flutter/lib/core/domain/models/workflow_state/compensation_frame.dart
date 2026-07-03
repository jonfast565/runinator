// port of core/domain/models/workflow-state/compensation-frame.ts.
//
// `state.compensation` saga-rollback bookkeeping.

import 'coerce_helpers.dart';

class CompensationFrame {
  const CompensationFrame({this.remaining, this.activeRunId});

  /// mirrors coerceCompensationFrame: returns null for an empty record.
  static CompensationFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    if (record.isEmpty) {
      return null;
    }

    return CompensationFrame(
      remaining: stringArrayOrNull(record['remaining']),
      activeRunId: record['active_run_id'] is String ? record['active_run_id'] as String : null,
    );
  }

  final List<String>? remaining;
  final String? activeRunId;

  Map<String, Object?> toJson() => {'remaining': remaining, 'active_run_id': activeRunId};
}
