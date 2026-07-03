// port of core/domain/models/run/run-summary.ts.

import '../../json.dart';

class RunSummary {
  const RunSummary({
    required this.id,
    this.workflowId,
    this.workflowSnapshot,
    required this.status,
    this.parameters,
    this.outputJson,
    this.message,
    this.trigger,
    required this.createdAt,
    required this.startedAt,
    required this.finishedAt,
    this.workflowRunId,
    this.workflowNodeId,
    this.activeNodeId,
    this.state,
    this.name,
  });

  factory RunSummary.fromJson(Map<String, Object?> json) => RunSummary(
        id: json['id'] as String,
        workflowId: json['workflow_id'] as String?,
        workflowSnapshot:
            json['workflow_snapshot'] != null ? asJsonObject(json['workflow_snapshot']) : null,
        status: json['status'] as String,
        parameters: json['parameters'] != null ? asJsonObject(json['parameters']) : null,
        outputJson: json.containsKey('output_json') ? asJsonValue(json['output_json']) : null,
        message: json['message'] as String?,
        trigger: json['trigger'] as String?,
        createdAt: json['created_at'] as String,
        startedAt: json['started_at'] as String?,
        finishedAt: json['finished_at'] as String?,
        workflowRunId: json['workflow_run_id'] as String?,
        workflowNodeId: json['workflow_node_id'] as String?,
        activeNodeId: json['active_node_id'] as String?,
        state: json['state'] != null ? asJsonObject(json['state']) : null,
        name: json['name'] as String?,
      );

  final String id;
  final String? workflowId;
  final JsonObject? workflowSnapshot;
  final String status;
  final JsonObject? parameters;
  final JsonValue outputJson;
  final String? message;
  final String? trigger;
  final String createdAt;
  final String? startedAt;
  final String? finishedAt;
  final String? workflowRunId;
  final String? workflowNodeId;
  final String? activeNodeId;
  final JsonObject? state;
  final String? name;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_id': workflowId,
        'workflow_snapshot': workflowSnapshot,
        'status': status,
        'parameters': parameters,
        'output_json': outputJson,
        'message': message,
        'trigger': trigger,
        'created_at': createdAt,
        'started_at': startedAt,
        'finished_at': finishedAt,
        'workflow_run_id': workflowRunId,
        'workflow_node_id': workflowNodeId,
        'active_node_id': activeNodeId,
        'state': state,
        'name': name,
      };
}
