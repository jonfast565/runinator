// port of core/domain/models/workflow/bundle.ts.

import 'definition.dart';
import 'trigger.dart';

class WorkflowBundle {
  const WorkflowBundle({required this.workflows, required this.triggers});

  factory WorkflowBundle.fromJson(Map<String, Object?> json) => WorkflowBundle(
        workflows: (json['workflows'] as List)
            .map((w) => WorkflowDefinition.fromJson(w as Map<String, Object?>))
            .toList(),
        triggers: (json['triggers'] as List)
            .map((t) => WorkflowTrigger.fromJson(t as Map<String, Object?>))
            .toList(),
      );

  final List<WorkflowDefinition> workflows;
  final List<WorkflowTrigger> triggers;

  Map<String, Object?> toJson() => {
        'workflows': workflows.map((w) => w.toJson()).toList(),
        'triggers': triggers.map((t) => t.toJson()).toList(),
      };
}
