// port of core/domain/models/workflow/layout.ts.

enum WorkflowLayoutDirection {
  horizontal('horizontal'),
  vertical('vertical');

  const WorkflowLayoutDirection(this.wire);

  final String wire;

  String toJson() => wire;
}

class WorkflowLayoutPosition {
  const WorkflowLayoutPosition({required this.x, required this.y});

  factory WorkflowLayoutPosition.fromJson(Map<String, Object?> json) => WorkflowLayoutPosition(
        x: (json['x'] as num).toDouble(),
        y: (json['y'] as num).toDouble(),
      );

  final double x;
  final double y;

  Map<String, Object?> toJson() => {'x': x, 'y': y};
}
