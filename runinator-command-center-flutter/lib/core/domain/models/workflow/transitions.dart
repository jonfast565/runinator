// port of core/domain/models/workflow/transitions.ts.

enum WorkflowDirectTransitionKey {
  next('next'),
  onSuccess('on_success'),
  onFailure('on_failure'),
  onTimeout('on_timeout'),
  onReject('on_reject');

  const WorkflowDirectTransitionKey(this.wire);

  final String wire;

  static WorkflowDirectTransitionKey fromJson(String value) =>
      WorkflowDirectTransitionKey.values.firstWhere(
        (key) => key.wire == value,
        orElse: () => throw ArgumentError('unknown WorkflowDirectTransitionKey: $value'),
      );

  String toJson() => wire;
}

typedef WorkflowConnectionHandle = String;
