// port of core/domain/models/workflow-state/parallel-frame.ts.
//
// `state.parallel` fan-out bookkeeping.

import 'coerce_helpers.dart';

class ParallelFrame {
  const ParallelFrame({required this.nodeId, this.remaining});

  /// mirrors coerceParallelFrame: returns null unless node_id is a string.
  static ParallelFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    final nodeId = record['node_id'];
    if (nodeId is! String) {
      return null;
    }

    return ParallelFrame(nodeId: nodeId, remaining: stringArrayOrNull(record['remaining']));
  }

  final String nodeId;
  final List<String>? remaining;

  Map<String, Object?> toJson() => {'node_id': nodeId, 'remaining': remaining};
}
