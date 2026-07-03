// port of core/domain/models/workflow-state/map-frame.ts.
//
// `state.map` bookkeeping (parent or child run).

import '../../json.dart';
import 'coerce_helpers.dart';

class MapChild {
  const MapChild({required this.index, required this.childRunId});

  factory MapChild.fromJson(Map<String, Object?> json) => MapChild(
        index: (json['index'] as num).toInt(),
        childRunId: json['child_run_id'] as String,
      );

  final int index;
  final String childRunId;

  Map<String, Object?> toJson() => {'index': index, 'child_run_id': childRunId};
}

class MapFrame {
  const MapFrame({
    required this.nodeId,
    required this.target,
    this.items,
    this.concurrency,
    this.nextIndex,
    this.inFlight,
    this.results,
    this.done,
    this.item,
    this.index,
  });

  /// mirrors coerceMapFrame: returns null unless node_id and target are strings.
  /// note the source's coerce function does not populate in_flight/results even
  /// though the type declares them — this is preserved verbatim, not a gap in the port.
  static MapFrame? fromCoercedJson(Object? value) {
    final record = coerceRecord(value);
    final nodeId = record['node_id'];
    final target = record['target'];
    if (nodeId is! String || target is! String) {
      return null;
    }

    final items = record['items'];

    return MapFrame(
      nodeId: nodeId,
      target: target,
      items: items is List ? items.map(asJsonValue).toList() : null,
      concurrency: record['concurrency'] is num ? (record['concurrency'] as num).toInt() : null,
      nextIndex: record['next_index'] is num ? (record['next_index'] as num).toInt() : null,
      done: record['done'] is num ? (record['done'] as num).toInt() : null,
      item: optionalJsonValue(record['item']),
      index: record['index'] is num ? (record['index'] as num).toInt() : null,
    );
  }

  final String nodeId;
  final String target;
  final List<JsonValue>? items;
  final int? concurrency;
  final int? nextIndex;
  final List<MapChild>? inFlight;
  final List<JsonValue>? results;
  final int? done;
  final JsonValue item;
  final int? index;

  Map<String, Object?> toJson() => {
        'node_id': nodeId,
        'target': target,
        'items': items,
        'concurrency': concurrency,
        'next_index': nextIndex,
        'in_flight': inFlight?.map((c) => c.toJson()).toList(),
        'results': results,
        'done': done,
        'item': item,
        'index': index,
      };
}
