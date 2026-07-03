// port of core/domain/models/workflow-state/debug-frame.ts.
//
// `state.debug` wire shape. config and runtime are flattened in json (see
// runinator-models::DebugFrame).

import '../../json.dart';
import '../workflow/node_kind.dart';
import 'coerce_helpers.dart';
import 'debug_mode.dart';

class DebugFrame {
  const DebugFrame({
    this.enabled,
    this.mode,
    this.breakpoints,
    this.paused,
    this.stepRequested,
    this.oneShotBreakpoint,
    this.currentNodeId,
    this.currentNodeKind,
    this.inputJson,
    this.contextJson,
    this.lastOutputJson,
  });

  /// mirrors coerceDebugFrame: returns null for a missing/empty record.
  static DebugFrame? fromCoercedJson(Object? value) {
    if (value == null) {
      return null;
    }

    final record = coerceRecord(value);
    if (record.isEmpty) {
      return null;
    }

    return DebugFrame(
      enabled: record['enabled'] is bool ? record['enabled'] as bool : null,
      mode: debugModeOrNull(record['mode']),
      breakpoints: stringArrayOrNull(record['breakpoints']),
      paused: record['paused'] is bool ? record['paused'] as bool : null,
      stepRequested: record['step_requested'] is bool ? record['step_requested'] as bool : null,
      oneShotBreakpoint:
          record['one_shot_breakpoint'] is String ? record['one_shot_breakpoint'] as String : null,
      currentNodeId: record['current_node_id'] is String ? record['current_node_id'] as String : null,
      currentNodeKind: workflowNodeKindOrNull(record['current_node_kind']),
      inputJson: optionalJsonValue(record['input_json']),
      contextJson: optionalJsonValue(record['context_json']),
      lastOutputJson: optionalJsonValue(record['last_output_json']),
    );
  }

  final bool? enabled;
  final DebugMode? mode;
  final List<String>? breakpoints;
  final bool? paused;
  final bool? stepRequested;
  final String? oneShotBreakpoint;
  final String? currentNodeId;
  final WorkflowNodeKind? currentNodeKind;
  final JsonValue inputJson;
  final JsonValue contextJson;
  final JsonValue lastOutputJson;

  Map<String, Object?> toJson() => {
        'enabled': enabled,
        'mode': mode?.toJson(),
        'breakpoints': breakpoints,
        'paused': paused,
        'step_requested': stepRequested,
        'one_shot_breakpoint': oneShotBreakpoint,
        'current_node_id': currentNodeId,
        'current_node_kind': currentNodeKind?.toJson(),
        'input_json': inputJson,
        'context_json': contextJson,
        'last_output_json': lastOutputJson,
      };
}
