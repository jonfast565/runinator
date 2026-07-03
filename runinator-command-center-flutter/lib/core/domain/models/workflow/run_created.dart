// port of core/domain/models/workflow/run-created.ts.

class WorkflowRunCreated {
  const WorkflowRunCreated({required this.id});

  factory WorkflowRunCreated.fromJson(Map<String, Object?> json) =>
      WorkflowRunCreated(id: json['id'] as String);

  final String id;

  Map<String, Object?> toJson() => {'id': id};
}
