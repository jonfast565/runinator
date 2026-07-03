// port of core/domain/models/workflow/node-ref.ts.

typedef WorkflowNodeId = String;

class WorkflowNodeRef {
  const WorkflowNodeRef(this.node);

  factory WorkflowNodeRef.fromJson(Map<String, Object?> json) =>
      WorkflowNodeRef(json[r'$node'] as WorkflowNodeId);

  final WorkflowNodeId node;

  Map<String, Object?> toJson() => {r'$node': node};
}

typedef WorkflowPathSegment = Object; // String or num
