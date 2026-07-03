// port of core/domain/models/workflow-state/loop-frame.ts.
//
// `state.loop` iteration bookkeeping for a loop body.

import '../../json.dart';
import 'coerce_helpers.dart';

class LoopFrame {
  const LoopFrame({this.index, this.item, this.returnTo});

  /// mirrors coerceLoopFrame: returns null for an empty record.
  static LoopFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    if (record.isEmpty) {
      return null;
    }

    return LoopFrame(
      index: record['index'] is num ? (record['index'] as num).toInt() : null,
      item: optionalJsonValue(record['item']),
      returnTo: record['return_to'] is String ? record['return_to'] as String : null,
    );
  }

  final int? index;
  final JsonValue item;
  final String? returnTo;

  Map<String, Object?> toJson() => {'index': index, 'item': item, 'return_to': returnTo};
}
