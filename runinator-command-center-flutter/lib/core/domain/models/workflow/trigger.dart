// port of core/domain/models/workflow/trigger.ts.

import '../../json.dart';

enum WorkflowTriggerKind {
  cron('cron'),
  manual('manual');

  const WorkflowTriggerKind(this.wire);

  final String wire;

  static WorkflowTriggerKind fromJson(String value) => WorkflowTriggerKind.values.firstWhere(
        (kind) => kind.wire == value,
        orElse: () => throw ArgumentError('unknown WorkflowTriggerKind: $value'),
      );

  String toJson() => wire;
}

class WorkflowTrigger {
  const WorkflowTrigger({
    required this.id,
    required this.workflowId,
    required this.kind,
    required this.enabled,
    required this.configuration,
    required this.nextExecution,
    required this.blackoutStart,
    required this.blackoutEnd,
    required this.metadata,
    this.createdAt,
    this.updatedAt,
  });

  factory WorkflowTrigger.fromJson(Map<String, Object?> json) => WorkflowTrigger(
        id: json['id'] as String?,
        workflowId: json['workflow_id'] as String,
        kind: WorkflowTriggerKind.fromJson(json['kind'] as String),
        enabled: json['enabled'] as bool,
        configuration: asJsonObject(json['configuration']),
        nextExecution: json['next_execution'] as String?,
        blackoutStart: json['blackout_start'] as String?,
        blackoutEnd: json['blackout_end'] as String?,
        metadata: asJsonObject(json['metadata']),
        createdAt: json['created_at'] as String?,
        updatedAt: json['updated_at'] as String?,
      );

  final String? id;
  final String workflowId;
  final WorkflowTriggerKind kind;
  final bool enabled;
  final JsonObject configuration;
  final String? nextExecution;
  final String? blackoutStart;
  final String? blackoutEnd;
  final JsonObject metadata;
  final String? createdAt;
  final String? updatedAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_id': workflowId,
        'kind': kind.toJson(),
        'enabled': enabled,
        'configuration': configuration,
        'next_execution': nextExecution,
        'blackout_start': blackoutStart,
        'blackout_end': blackoutEnd,
        'metadata': metadata,
        'created_at': createdAt,
        'updated_at': updatedAt,
      };
}
