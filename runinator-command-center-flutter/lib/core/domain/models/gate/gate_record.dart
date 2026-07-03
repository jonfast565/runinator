// port of core/domain/models/gate/gate-record.ts.

import '../../json.dart';
import 'gate_kind.dart';

class GateRecord {
  const GateRecord({
    this.id,
    required this.workflowRunId,
    required this.nodeId,
    required this.kind,
    required this.status,
    this.label,
    this.condition,
    this.reason,
    this.resolvedBy,
    this.resolvedAt,
    this.metadata,
    this.createdAt,
    this.updatedAt,
  });

  factory GateRecord.fromJson(Map<String, Object?> json) => GateRecord(
        id: json['id'] as String?,
        workflowRunId: json['workflow_run_id'] as String,
        nodeId: json['node_id'] as String,
        kind: GateKind.fromJson(json['kind'] as String),
        status: json['status'] as String,
        label: json['label'] as String?,
        condition: json.containsKey('condition') ? asJsonValue(json['condition']) : null,
        reason: json['reason'] as String?,
        resolvedBy: json['resolved_by'] as String?,
        resolvedAt: json['resolved_at'] as String?,
        metadata: json['metadata'] != null ? asJsonObject(json['metadata']) : null,
        createdAt: json['created_at'] as String?,
        updatedAt: json['updated_at'] as String?,
      );

  final String? id;
  final String workflowRunId;
  final String nodeId;
  final GateKind kind;
  final String status;
  final String? label;
  final JsonValue condition;
  final String? reason;
  final String? resolvedBy;
  final String? resolvedAt;
  final JsonObject? metadata;
  final String? createdAt;
  final String? updatedAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_run_id': workflowRunId,
        'node_id': nodeId,
        'kind': kind.toJson(),
        'status': status,
        'label': label,
        'condition': condition,
        'reason': reason,
        'resolved_by': resolvedBy,
        'resolved_at': resolvedAt,
        'metadata': metadata,
        'created_at': createdAt,
        'updated_at': updatedAt,
      };
}
