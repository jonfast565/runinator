// port of core/domain/models/workflow/validation.ts.

enum WorkflowValidationSeverity {
  error('error'),
  warning('warning');

  const WorkflowValidationSeverity(this.wire);

  final String wire;

  static WorkflowValidationSeverity fromJson(String value) =>
      WorkflowValidationSeverity.values.firstWhere(
        (severity) => severity.wire == value,
        orElse: () => throw ArgumentError('unknown WorkflowValidationSeverity: $value'),
      );

  String toJson() => wire;
}

class WorkflowValidationIssue {
  const WorkflowValidationIssue({
    required this.severity,
    required this.message,
    required this.nodeId,
    this.edgeKey,
  });

  factory WorkflowValidationIssue.fromJson(Map<String, Object?> json) => WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.fromJson(json['severity'] as String),
        message: json['message'] as String,
        nodeId: json['nodeId'] as String,
        edgeKey: json['edgeKey'] as String?,
      );

  final WorkflowValidationSeverity severity;
  final String message;
  final String nodeId;
  final String? edgeKey;

  Map<String, Object?> toJson() => {
        'severity': severity.toJson(),
        'message': message,
        'nodeId': nodeId,
        'edgeKey': edgeKey,
      };
}

enum WorkflowInlineEditValueKind {
  text('text'),
  number('number');

  const WorkflowInlineEditValueKind(this.wire);

  final String wire;

  String toJson() => wire;
}

class WorkflowInlineEditDescriptor {
  const WorkflowInlineEditDescriptor({
    required this.label,
    required this.value,
    required this.valueKind,
  });

  factory WorkflowInlineEditDescriptor.fromJson(Map<String, Object?> json) =>
      WorkflowInlineEditDescriptor(
        label: json['label'] as String,
        value: json['value'] as String,
        valueKind: (json['valueKind'] as String) == 'number'
            ? WorkflowInlineEditValueKind.number
            : WorkflowInlineEditValueKind.text,
      );

  final String label;
  final String value;
  final WorkflowInlineEditValueKind valueKind;

  Map<String, Object?> toJson() => {
        'label': label,
        'value': value,
        'valueKind': valueKind.toJson(),
      };
}
