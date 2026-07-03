// port of core/domain/models/workflow/edge.ts.

import 'transitions.dart';
import 'validation.dart';

/// sentinel used by [WorkflowEdgeEditorDraft.copyWith] to distinguish "not passed"
/// from "explicitly passed null" for nullable fields.
const Object _unset = Object();

enum WorkflowEditorEdgeKind {
  direct('direct'),
  branch('branch'),
  control('control');

  const WorkflowEditorEdgeKind(this.wire);

  final String wire;

  static WorkflowEditorEdgeKind fromJson(String value) =>
      WorkflowEditorEdgeKind.values.firstWhere((kind) => kind.wire == value);

  String toJson() => wire;
}

enum WorkflowEdgeStyle {
  bezier('bezier'),
  straight('straight'),
  square('square');

  const WorkflowEdgeStyle(this.wire);

  final String wire;

  static WorkflowEdgeStyle fromJson(String value) =>
      WorkflowEdgeStyle.values.firstWhere((style) => style.wire == value);

  String toJson() => wire;
}

class WorkflowEdgeSemanticOption {
  const WorkflowEdgeSemanticOption({
    required this.id,
    required this.label,
    required this.description,
  });

  factory WorkflowEdgeSemanticOption.fromJson(Map<String, Object?> json) =>
      WorkflowEdgeSemanticOption(
        id: json['id'] as String,
        label: json['label'] as String,
        description: json['description'] as String,
      );

  final String id;
  final String label;
  final String description;

  Map<String, Object?> toJson() => {'id': id, 'label': label, 'description': description};
}

enum WorkflowSemanticHandleType {
  source('source'),
  target('target');

  const WorkflowSemanticHandleType(this.wire);

  final String wire;

  static WorkflowSemanticHandleType fromJson(String value) =>
      WorkflowSemanticHandleType.values.firstWhere((type) => type.wire == value);

  String toJson() => wire;
}

class WorkflowSemanticHandle {
  const WorkflowSemanticHandle({
    required this.id,
    required this.label,
    required this.type,
    this.semanticOptionId,
  });

  factory WorkflowSemanticHandle.fromJson(Map<String, Object?> json) => WorkflowSemanticHandle(
        id: json['id'] as String,
        label: json['label'] as String,
        type: WorkflowSemanticHandleType.fromJson(json['type'] as String),
        semanticOptionId: json['semanticOptionId'] as String?,
      );

  final String id;
  final String label;
  final WorkflowSemanticHandleType type;
  final String? semanticOptionId;

  Map<String, Object?> toJson() => {
        'id': id,
        'label': label,
        'type': type.toJson(),
        'semanticOptionId': semanticOptionId,
      };
}

enum WorkflowEdgeEditorMatchKind {
  equals('equals'),
  notEquals('not_equals'),
  exists('exists'),
  when('when');

  const WorkflowEdgeEditorMatchKind(this.wire);

  final String wire;

  static WorkflowEdgeEditorMatchKind fromJson(String value) =>
      WorkflowEdgeEditorMatchKind.values.firstWhere((kind) => kind.wire == value);

  String toJson() => wire;
}

class WorkflowEdgeEditorDraft {
  const WorkflowEdgeEditorDraft({
    required this.edgeId,
    required this.source,
    required this.target,
    required this.optionId,
    this.sourceHandle,
    this.targetHandle,
    required this.edgeStyle,
    required this.labelAnchor,
    required this.label,
    required this.whenJson,
    required this.matchKind,
    required this.matchJson,
    required this.canEditLabel,
    required this.canEditCondition,
    required this.canEditSwitchCase,
    required this.canMove,
    required this.orderIndex,
    required this.orderCount,
    required this.priority,
    required this.canEditPriority,
  });

  factory WorkflowEdgeEditorDraft.fromJson(Map<String, Object?> json) => WorkflowEdgeEditorDraft(
        edgeId: json['edgeId'] as String,
        source: json['source'] as String,
        target: json['target'] as String,
        optionId: json['optionId'] as String,
        sourceHandle: json['sourceHandle'] as WorkflowConnectionHandle?,
        targetHandle: json['targetHandle'] as WorkflowConnectionHandle?,
        edgeStyle: WorkflowEdgeStyle.fromJson(json['edgeStyle'] as String),
        labelAnchor: (json['labelAnchor'] as num).toDouble(),
        label: json['label'] as String,
        whenJson: json['whenJson'] as String,
        matchKind: WorkflowEdgeEditorMatchKind.fromJson(json['matchKind'] as String),
        matchJson: json['matchJson'] as String,
        canEditLabel: json['canEditLabel'] as bool,
        canEditCondition: json['canEditCondition'] as bool,
        canEditSwitchCase: json['canEditSwitchCase'] as bool,
        canMove: json['canMove'] as bool,
        orderIndex: json['orderIndex'] as int,
        orderCount: json['orderCount'] as int,
        priority: (json['priority'] as num?)?.toInt(),
        canEditPriority: json['canEditPriority'] as bool,
      );

  final String edgeId;
  final String source;
  final String target;
  final String optionId;
  final WorkflowConnectionHandle? sourceHandle;
  final WorkflowConnectionHandle? targetHandle;
  final WorkflowEdgeStyle edgeStyle;
  final double labelAnchor;
  final String label;
  final String whenJson;
  final WorkflowEdgeEditorMatchKind matchKind;
  final String matchJson;
  final bool canEditLabel;
  final bool canEditCondition;
  final bool canEditSwitchCase;
  final bool canMove;
  final int orderIndex;
  final int orderCount;

  /// selection priority for predicate edges; lower numbers are evaluated first. null means unset.
  final int? priority;
  final bool canEditPriority;

  WorkflowEdgeEditorDraft copyWith({
    String? edgeId,
    String? source,
    String? target,
    String? optionId,
    Object? sourceHandle = _unset,
    Object? targetHandle = _unset,
    WorkflowEdgeStyle? edgeStyle,
    double? labelAnchor,
    String? label,
    String? whenJson,
    WorkflowEdgeEditorMatchKind? matchKind,
    String? matchJson,
    bool? canEditLabel,
    bool? canEditCondition,
    bool? canEditSwitchCase,
    bool? canMove,
    int? orderIndex,
    int? orderCount,
    Object? priority = _unset,
    bool? canEditPriority,
  }) =>
      WorkflowEdgeEditorDraft(
        edgeId: edgeId ?? this.edgeId,
        source: source ?? this.source,
        target: target ?? this.target,
        optionId: optionId ?? this.optionId,
        sourceHandle:
            identical(sourceHandle, _unset) ? this.sourceHandle : sourceHandle as WorkflowConnectionHandle?,
        targetHandle:
            identical(targetHandle, _unset) ? this.targetHandle : targetHandle as WorkflowConnectionHandle?,
        edgeStyle: edgeStyle ?? this.edgeStyle,
        labelAnchor: labelAnchor ?? this.labelAnchor,
        label: label ?? this.label,
        whenJson: whenJson ?? this.whenJson,
        matchKind: matchKind ?? this.matchKind,
        matchJson: matchJson ?? this.matchJson,
        canEditLabel: canEditLabel ?? this.canEditLabel,
        canEditCondition: canEditCondition ?? this.canEditCondition,
        canEditSwitchCase: canEditSwitchCase ?? this.canEditSwitchCase,
        canMove: canMove ?? this.canMove,
        orderIndex: orderIndex ?? this.orderIndex,
        orderCount: orderCount ?? this.orderCount,
        priority: identical(priority, _unset) ? this.priority : priority as int?,
        canEditPriority: canEditPriority ?? this.canEditPriority,
      );

  Map<String, Object?> toJson() => {
        'edgeId': edgeId,
        'source': source,
        'target': target,
        'optionId': optionId,
        'sourceHandle': sourceHandle,
        'targetHandle': targetHandle,
        'edgeStyle': edgeStyle.toJson(),
        'labelAnchor': labelAnchor,
        'label': label,
        'whenJson': whenJson,
        'matchKind': matchKind.toJson(),
        'matchJson': matchJson,
        'canEditLabel': canEditLabel,
        'canEditCondition': canEditCondition,
        'canEditSwitchCase': canEditSwitchCase,
        'canMove': canMove,
        'orderIndex': orderIndex,
        'orderCount': orderCount,
        'priority': priority,
        'canEditPriority': canEditPriority,
      };
}

class WorkflowEdgeLabelOffset {
  const WorkflowEdgeLabelOffset({required this.x, required this.y});

  factory WorkflowEdgeLabelOffset.fromJson(Map<String, Object?> json) => WorkflowEdgeLabelOffset(
        x: (json['x'] as num).toDouble(),
        y: (json['y'] as num).toDouble(),
      );

  final double x;
  final double y;

  Map<String, Object?> toJson() => {'x': x, 'y': y};
}

class WorkflowEdgeLabelAnchor {
  const WorkflowEdgeLabelAnchor({required this.position});

  factory WorkflowEdgeLabelAnchor.fromJson(Map<String, Object?> json) =>
      WorkflowEdgeLabelAnchor(position: (json['position'] as num).toDouble());

  final double position;

  Map<String, Object?> toJson() => {'position': position};
}

class WorkflowEditorEdgeData {
  const WorkflowEditorEdgeData({
    required this.kind,
    this.transitionKey,
    this.branchIndex,
    this.parameterKey,
    this.parameterIndex,
    this.sourceHandle,
    this.targetHandle,
    this.edgeStyle,
    this.labelOffset,
    this.labelAnchor,
    this.parallelOffset,
    this.validationCount,
    this.validationSeverity,
    this.validationMessages,
    required this.editable,
  });

  factory WorkflowEditorEdgeData.fromJson(Map<String, Object?> json) => WorkflowEditorEdgeData(
        kind: WorkflowEditorEdgeKind.fromJson(json['kind'] as String),
        transitionKey: json['transitionKey'] != null
            ? WorkflowDirectTransitionKey.fromJson(json['transitionKey'] as String)
            : null,
        branchIndex: (json['branchIndex'] as num?)?.toInt(),
        parameterKey: json['parameterKey'] as String?,
        parameterIndex: (json['parameterIndex'] as num?)?.toInt(),
        sourceHandle: json['sourceHandle'] as WorkflowConnectionHandle?,
        targetHandle: json['targetHandle'] as WorkflowConnectionHandle?,
        edgeStyle:
            json['edgeStyle'] != null ? WorkflowEdgeStyle.fromJson(json['edgeStyle'] as String) : null,
        labelOffset: json['labelOffset'] != null
            ? WorkflowEdgeLabelOffset.fromJson(json['labelOffset'] as Map<String, Object?>)
            : null,
        labelAnchor: json['labelAnchor'] != null
            ? WorkflowEdgeLabelAnchor.fromJson(json['labelAnchor'] as Map<String, Object?>)
            : null,
        parallelOffset: (json['parallelOffset'] as num?)?.toDouble(),
        validationCount: (json['validationCount'] as num?)?.toInt(),
        validationSeverity: json['validationSeverity'] != null
            ? WorkflowValidationSeverity.fromJson(json['validationSeverity'] as String)
            : null,
        validationMessages: (json['validationMessages'] as List?)?.cast<String>(),
        editable: json['editable'] as bool,
      );

  final WorkflowEditorEdgeKind kind;
  final WorkflowDirectTransitionKey? transitionKey;
  final int? branchIndex;
  final String? parameterKey;
  final int? parameterIndex;
  final WorkflowConnectionHandle? sourceHandle;
  final WorkflowConnectionHandle? targetHandle;
  final WorkflowEdgeStyle? edgeStyle;
  final WorkflowEdgeLabelOffset? labelOffset;
  final WorkflowEdgeLabelAnchor? labelAnchor;
  final double? parallelOffset;
  final int? validationCount;
  final WorkflowValidationSeverity? validationSeverity;
  final List<String>? validationMessages;
  final bool editable;

  Map<String, Object?> toJson() => {
        'kind': kind.toJson(),
        'transitionKey': transitionKey?.toJson(),
        'branchIndex': branchIndex,
        'parameterKey': parameterKey,
        'parameterIndex': parameterIndex,
        'sourceHandle': sourceHandle,
        'targetHandle': targetHandle,
        'edgeStyle': edgeStyle?.toJson(),
        'labelOffset': labelOffset?.toJson(),
        'labelAnchor': labelAnchor?.toJson(),
        'parallelOffset': parallelOffset,
        'validationCount': validationCount,
        'validationSeverity': validationSeverity?.toJson(),
        'validationMessages': validationMessages,
        'editable': editable,
      };
}
