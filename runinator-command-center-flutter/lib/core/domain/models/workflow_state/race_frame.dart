// port of core/domain/models/workflow-state/race-frame.ts.
//
// `state.race` fan-out bookkeeping.

import 'coerce_helpers.dart';

class RaceFrame {
  const RaceFrame({required this.nodeId, this.remaining});

  /// mirrors coerceRaceFrame: returns null unless node_id is a string.
  static RaceFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    final nodeId = record['node_id'];
    if (nodeId is! String) {
      return null;
    }

    return RaceFrame(nodeId: nodeId, remaining: stringArrayOrNull(record['remaining']));
  }

  final String nodeId;
  final List<String>? remaining;

  Map<String, Object?> toJson() => {'node_id': nodeId, 'remaining': remaining};
}
