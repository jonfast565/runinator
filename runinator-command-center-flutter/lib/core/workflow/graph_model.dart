// port of core/workflow/graph-model.ts.

import '../domain/models/index.dart';

class GraphNodeData {
  const GraphNodeData({
    required this.title,
    required this.nodeId,
    required this.kind,
    required this.summary,
    required this.semanticHandles,
    required this.inlineEdit,
    required this.validationIssues,
    required this.validationCount,
    this.validationSeverity,
    this.statusLabel,
    required this.executionCount,
    this.approvalPrompt,
    this.inputPrompt,
    required this.running,
    this.status,
    required this.protected_,
    required this.locked,
    required this.skipped,
    required this.debugBreakpoint,
  });

  final String title;
  final String nodeId;
  final String kind;
  final String summary;
  final List<WorkflowSemanticHandle> semanticHandles;
  final WorkflowInlineEditDescriptor? inlineEdit;
  final List<WorkflowValidationIssue> validationIssues;
  final int validationCount;
  final WorkflowValidationSeverity? validationSeverity;
  final String? statusLabel;
  final int executionCount;
  final String? approvalPrompt;
  final String? inputPrompt;
  final bool running;
  final String? status;
  final bool protected_;
  final bool locked;
  final bool skipped;
  final bool debugBreakpoint;
}

class GraphPosition {
  const GraphPosition({required this.x, required this.y});

  final double x;
  final double y;
}

class GraphNodeModel {
  const GraphNodeModel({
    required this.id,
    required this.type,
    required this.position,
    required this.data,
    this.className,
  });

  final String id;
  final String type;
  final GraphPosition position;
  final GraphNodeData data;
  final String? className;
}

class GraphEdgePathOptions {
  const GraphEdgePathOptions({this.offset, this.borderRadius});

  final double? offset;
  final double? borderRadius;
}

/// fields read by portable edge editor helpers; compatible with a future Flutter graph canvas.
class GraphEdgeLike {
  const GraphEdgeLike({
    this.id,
    required this.source,
    required this.target,
    this.sourceHandle,
    this.targetHandle,
    this.data,
  });

  final String? id;
  final String source;
  final String target;
  final String? sourceHandle;
  final String? targetHandle;
  final WorkflowEditorEdgeData? data;
}

class GraphEdgeModel extends GraphEdgeLike {
  const GraphEdgeModel({
    required super.id,
    required super.source,
    required super.target,
    super.sourceHandle,
    super.targetHandle,
    required WorkflowEditorEdgeData data,
    required this.type,
    this.label,
    this.updatable,
    this.interactionWidth,
    this.pathOptions,
    this.zIndex,
  }) : super(data: data);

  final String type;
  final String? label;

  /// mirrors the ts source's `boolean | string`.
  final Object? updatable;
  final double? interactionWidth;
  final GraphEdgePathOptions? pathOptions;
  final int? zIndex;

  @override
  WorkflowEditorEdgeData get data => super.data!;
}
