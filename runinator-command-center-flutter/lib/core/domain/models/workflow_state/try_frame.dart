// port of core/domain/models/workflow-state/try-frame.ts.
//
// `state.try` / try node-run phase bookkeeping.

import '../../json.dart';
import 'coerce_helpers.dart';

class TryFrame {
  const TryFrame({
    required this.nodeId,
    required this.phase,
    this.pendingStatus,
    this.pendingOutput,
  });

  /// mirrors coerceTryFrame: returns null unless node_id and phase are strings.
  static TryFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    final nodeId = record['node_id'];
    final phase = record['phase'];
    if (nodeId is! String || phase is! String) {
      return null;
    }

    return TryFrame(
      nodeId: nodeId,
      phase: phase,
      pendingStatus: record['pending_status'] is String ? record['pending_status'] as String : null,
      pendingOutput: optionalJsonValue(record['pending_output']),
    );
  }

  final String nodeId;
  final String phase;
  final String? pendingStatus;
  final JsonValue pendingOutput;

  Map<String, Object?> toJson() => {
        'node_id': nodeId,
        'phase': phase,
        'pending_status': pendingStatus,
        'pending_output': pendingOutput,
      };
}
