// port of core/workflow/editor-defaults.ts.

import 'dart:convert';

import '../domain/json.dart';
import '../domain/models/index.dart';
import '../utils/format.dart';
import '../utils/values.dart';
import 'graph_model.dart';
import 'workflow_helpers.dart' show asRecord, asArray, nodeRef, nodeRefId, valueRef;

final Set<String> _protectedWorkflowNodeKinds = {'start', 'end', 'fail'};

WorkflowEdgeEditorDraft defaultEdgeEditorDraft() => WorkflowEdgeEditorDraft(
      edgeId: '',
      source: '',
      target: '',
      optionId: '',
      edgeStyle: WorkflowEdgeStyle.square,
      labelAnchor: 50,
      label: '',
      whenJson: pretty({
        'value': valueRef('params', ['value']),
        'equals': true,
      }),
      matchKind: WorkflowEdgeEditorMatchKind.equals,
      matchJson: pretty(true),
      canEditLabel: false,
      canEditCondition: false,
      canEditSwitchCase: false,
      canMove: false,
      orderIndex: -1,
      orderCount: 0,
      priority: null,
      canEditPriority: false,
    );

typedef BranchPolicyName = String; // 'all' | 'any' | 'first_success'

class SwitchCaseEditor {
  const SwitchCaseEditor({required this.matchKind, required this.matchJson, required this.target});

  final String matchKind;
  final String matchJson;
  final String target;
}

String branchPolicyName(Object? value, String fallback) =>
    (value == 'all' || value == 'any' || value == 'first_success') ? value as String : fallback;

SwitchCaseEditor switchCaseEditor(JsonRecord value) {
  final target = nodeRefId(value['target']) ?? '';

  if (value.containsKey('when')) {
    return SwitchCaseEditor(matchKind: 'when', matchJson: pretty(value['when']), target: target);
  }

  if (value.containsKey('not_equals')) {
    return SwitchCaseEditor(
      matchKind: 'not_equals',
      matchJson: pretty(value['not_equals']),
      target: target,
    );
  }

  if (value.containsKey('exists')) {
    return SwitchCaseEditor(
      matchKind: 'exists',
      matchJson: pretty(value['exists'] == true),
      target: target,
    );
  }

  return SwitchCaseEditor(matchKind: 'equals', matchJson: pretty(value['equals'] ?? ''), target: target);
}

WorkflowTrigger newWorkflowTriggerDraft(String workflowId, [WorkflowTriggerKind kind = WorkflowTriggerKind.cron]) =>
    WorkflowTrigger(
      id: null,
      workflowId: workflowId,
      kind: kind,
      enabled: true,
      configuration: defaultTriggerConfiguration(kind),
      nextExecution: null,
      blackoutStart: null,
      blackoutEnd: null,
      metadata: const {},
    );

JsonRecord defaultTriggerConfiguration(WorkflowTriggerKind kind) {
  if (kind == WorkflowTriggerKind.cron) {
    return {'cron': '0 * * * *', 'parameters': <String, Object?>{}};
  }

  return {};
}

/// seed a draft input object from the workflow's input struct so declared fields render pre-populated.
JsonRecord buildInputSkeleton(RuninatorType? ty) {
  if (ty is! RuninatorTypeStruct) {
    return {};
  }

  final skeleton = <String, Object?>{};

  for (final entry in ty.fields.entries) {
    skeleton[entry.key] = _defaultValueForInputType(entry.value.ty);
  }

  return skeleton;
}

JsonValue _defaultValueForInputType(RuninatorType ty) {
  switch (ty) {
    case RuninatorTypeString():
      return '';
    case RuninatorTypeBoolean():
      return false;
    case RuninatorTypeInteger():
    case RuninatorTypeDuration():
    case RuninatorTypeNumber():
      return 0;
    case RuninatorTypeEnum(:final values):
      return values.isNotEmpty ? values.first : null;
    case RuninatorTypeRange(:final min, :final base):
      return min ?? _defaultValueForInputType(base);
    case RuninatorTypeArray():
      return <Object?>[];
    case RuninatorTypeMap():
      return <String, Object?>{};
    case RuninatorTypeStruct():
      return asJsonValue(buildInputSkeleton(ty));
    case RuninatorTypeUnion(:final variants):
      return variants.isNotEmpty ? _defaultValueForInputType(variants.first) : null;
    default:
      return null;
  }
}

String? dateTimeLocalToIso(String? value) {
  if (value == null || value.isEmpty) {
    return null;
  }

  final date = DateTime.tryParse(value);
  return date?.toUtc().toIso8601String();
}

Map<String, Object?> createStepEditorState() => {
      'id': '',
      'name': '',
      'kind': 'action',
      'approval_type': 'generic',
      'approval_prompt': 'Approval required',
      'gate_kind': 'manual',
      'gate_when_json': '{}',
      'gate_poll_interval': 30,
      'gate_timeout': 0,
      'gate_label': '',
      'signal_name': 'signal',
      'condition_fallback': '',
      'condition_branches': <Map<String, String>>[],
      'wait_seconds': 60,
      'wait_initial_status': 'waiting',
      'wait_until_status': '',
      'wait_json': '{}',
      'loop_items_json': '[]',
      'loop_target': '',
      'loop_max_iterations': 10,
      'switch_value_json': pretty(valueRef('params', ['mode'])),
      'switch_cases': <SwitchCaseEditor>[],
      'switch_default': '',
      'toggle_value_json': pretty(valueRef('config', ['flags', 'enabled'])),
      'toggle_on': '',
      'toggle_off': '',
      'percentage_key_json': pretty(valueRef('input', ['user_id'])),
      'percentage_buckets': <Map<String, Object?>>[],
      'percentage_default': '',
      'parallel_branches': <String>[],
      'join_wait_for': <String>[],
      'join_mode': 'all',
      'try_body': '',
      'try_catch': '',
      'try_finally': '',
      'map_items_json': '[]',
      'map_target': '',
      'map_concurrency': 1,
      'race_branches': <String>[],
      'race_winner': 'first_success',
      'output_event_type': 'workflow.output',
      'output_data_json': '{}',
      'input_prompt': 'Provide input',
      'config_name_json': '""',
      'config_metadata_json': '{}',
      'subflow_id': '',
      'subflow_parameters_json': '{}',
      'assert_assertions': <Map<String, String>>[],
      'transform_bindings_json': '{}',
      'audit_action_json': pretty('workflow.audit'),
      'audit_actor_json': '',
      'audit_target_json': '',
      'audit_reason_json': '',
      'checkpoint_name': '',
      'mutex_name': '',
      'mutex_poll_interval': 30,
      'throttle_name': '',
      'throttle_max_per_window': 10,
      'throttle_window_seconds': 60,
      'throttle_poll_interval': 30,
      'await_run_ids_json': pretty(valueRef('params', ['run_ids'])),
      'await_mode': 'all',
      'await_poll_interval': 30,
      'debounce_name': '',
      'debounce_delay_seconds': 30,
      'debounce_trigger_key_json': '',
      'collect_name': '',
      'collect_max': 10,
      'barrier_name': '',
      'barrier_count': 2,
      'barrier_poll_interval': 30,
      'circuit_name': '',
      'circuit_threshold': 5,
      'circuit_window_seconds': 60,
      'circuit_cooldown_seconds': 60,
      'event_source_type': '*',
      'event_source_filter_json': '',
      'event_source_max': 0,
      'locked': false,
      'skipped': false,
      'max_attempts': 1,
      'timeout_seconds': 0,
      'action_name': '',
      'action_function': '',
      'parameters_json': '{}',
      'transitions_json': '{}',
    };

WorkflowDefinition newWorkflowDraft() => WorkflowDefinition(
      id: null,
      name: 'New Workflow',
      version: '1.0.0',
      enabled: true,
      inputType: const {
        'type': 'struct',
        'fields': <String, Object?>{},
        'additional': {'type': 'any'},
      },
      definition: const {
        'start': 'start',
        'nodes': [
          {'id': 'start', 'kind': 'start', 'transitions': <String, Object?>{}},
          {'id': 'end', 'kind': 'end'},
          {'id': 'fail', 'kind': 'fail'},
        ],
        'ui': {
          'layout': {
            'nodes': {
              'start': {'x': 0, 'y': 0},
              'end': {'x': 270, 'y': 0},
              'fail': {'x': 270, 'y': 150},
            },
          },
        },
      },
    );

int boundedIndex(int current, int delta, int length) {
  if (current < 0) {
    return delta > 0 ? 0 : length - 1;
  }

  return (current + delta).clamp(0, length - 1);
}

String formatMaybeDate(String? value) {
  if (value == null || value.isEmpty) {
    return '-';
  }

  final date = DateTime.tryParse(value);
  return date == null ? value : date.toLocal().toString();
}

void normalizeNewNodeTargets(JsonRecord node, String endId) {
  final transitions = asRecord(node['transitions']);
  node['transitions'] = transitions;

  for (final key in ['next', 'on_success', 'on_reject']) {
    if (nodeRefId(transitions[key]) == 'end') {
      transitions[key] = nodeRef(endId);
    }
  }

  for (final entry in asArray(transitions['branches'])) {
    final branch = asRecord(entry);

    if (nodeRefId(branch['target']) == 'end') {
      branch['target'] = nodeRef(endId);
    }
  }

  final parameters = asRecord(node['parameters']);

  if (nodeRefId(parameters['target']) == 'end') {
    parameters['target'] = nodeRef(endId);
    node['parameters'] = parameters;
  }

  if (nodeRefId(parameters['default']) == 'end') {
    parameters['default'] = nodeRef(endId);
    node['parameters'] = parameters;
  }
}

String validateJsonValueType(Object? value, RuninatorType? ty, String label) {
  if (ty == null || ty is RuninatorTypeAny || _isWorkflowExpression(value)) {
    return '';
  }

  if (ty is RuninatorTypeNull) {
    return value == null ? '' : '$label must be null';
  }

  if (ty is RuninatorTypeString) {
    return value is String ? '' : '$label must be a string';
  }

  if (ty is RuninatorTypeBoolean) {
    return value is bool ? '' : '$label must be true or false';
  }

  if (ty is RuninatorTypeInteger) {
    return (value is int || (value is num && value == value.roundToDouble()))
        ? ''
        : '$label must be an integer';
  }

  if (ty is RuninatorTypeNumber) {
    return (value is num && !value.isNaN) ? '' : '$label must be a number';
  }

  if (ty is RuninatorTypeDuration) {
    return (value is int || (value is num && value == value.roundToDouble()))
        ? ''
        : '$label must be a duration in seconds';
  }

  if (ty is RuninatorTypeEnum) {
    final encodedValue = jsonEncode(value);
    return ty.values.any((candidate) => jsonEncode(candidate) == encodedValue)
        ? ''
        : '$label must be one of ${ty.values.map(jsonEncode).join(", ")}';
  }

  if (ty is RuninatorTypeRange) {
    final baseError = validateJsonValueType(value, ty.base, label);

    if (baseError.isNotEmpty) {
      return baseError;
    }

    if (value is num && ty.min != null && value < ty.min!) {
      return '$label must be at least ${ty.min}';
    }

    if (value is num && ty.max != null && value > ty.max!) {
      return '$label must be at most ${ty.max}';
    }

    return '';
  }

  if (ty is RuninatorTypeArray) {
    if (value is! List) {
      return '$label must be a list';
    }

    for (var index = 0; index < value.length; index++) {
      final error = validateJsonValueType(value[index], ty.items, '$label[$index]');

      if (error.isNotEmpty) {
        return error;
      }
    }

    return '';
  }

  if (ty is RuninatorTypeMap) {
    if (!_isJsonRecord(value)) {
      return '$label must be an object';
    }

    for (final entry in (value as Map<String, Object?>).entries) {
      final error = validateJsonValueType(entry.value, ty.values, '$label.${entry.key}');

      if (error.isNotEmpty) {
        return error;
      }
    }

    return '';
  }

  if (ty is RuninatorTypeStruct) {
    if (!_isJsonRecord(value)) {
      return '$label must be an object';
    }

    final record = value as Map<String, Object?>;

    for (final entry in ty.fields.entries) {
      final nested = record[entry.key];

      if (isBlankValue(nested)) {
        if (entry.value.required) {
          return '$label.${entry.key} is required';
        }

        continue;
      }

      final error = validateJsonValueType(nested, entry.value.ty, '$label.${entry.key}');

      if (error.isNotEmpty) {
        return error;
      }
    }

    for (final entry in record.entries) {
      if (ty.fields.containsKey(entry.key)) {
        continue;
      }

      if (ty.additional == null) {
        return '$label.${entry.key} is not allowed';
      }

      final error = validateJsonValueType(entry.value, ty.additional, '$label.${entry.key}');

      if (error.isNotEmpty) {
        return error;
      }
    }

    return '';
  }

  final union = ty as RuninatorTypeUnion;
  return union.variants.any((variant) => validateJsonValueType(value, variant, label).isEmpty)
      ? ''
      : '$label must match one of ${union.variants.map(_runinatorTypeTag).join(", ")}';
}

String _runinatorTypeTag(RuninatorType ty) => ty.toJson()['type'] as String;

bool _isJsonRecord(Object? value) => value is Map<String, Object?>;

bool isProtectedWorkflowNode(JsonRecord? node) =>
    _protectedWorkflowNodeKinds.contains(displayValue(node?['kind']));

bool isLockedWorkflowNode(JsonRecord? node) =>
    isProtectedWorkflowNode(node) || node?['locked'] == true;

bool _isWorkflowExpression(Object? value) {
  if (!_isJsonRecord(value)) {
    return false;
  }

  final record = value as Map<String, Object?>;
  return [
    r'$ref',
    r'$concat',
    r'$coalesce',
    r'$literal',
    r'$to_string',
    r'$to_json_string',
    r'$node',
  ].any(record.containsKey);
}

GraphPosition nextNodePosition(int count) =>
    GraphPosition(x: ((count - 1) % 4) * 230, y: ((count - 1) / 4).floor() * 130);
