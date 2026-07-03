// port of core/domain/models/workflow/node-run.ts.

import '../../json.dart';

class WorkflowNodeRun {
  const WorkflowNodeRun({
    required this.id,
    required this.workflowRunId,
    required this.nodeId,
    required this.status,
    required this.attempt,
    required this.parameters,
    this.outputJson,
    this.state,
    this.transitionReason,
    this.prevNodeRunId,
    this.createdAt,
    this.startedAt,
    this.finishedAt,
    required this.message,
  });

  factory WorkflowNodeRun.fromJson(Map<String, Object?> json) => WorkflowNodeRun(
        id: json['id'] as String,
        workflowRunId: json['workflow_run_id'] as String,
        nodeId: json['node_id'] as String,
        status: json['status'] as String,
        attempt: (json['attempt'] as num).toInt(),
        parameters: asJsonObject(json['parameters']),
        outputJson: json.containsKey('output_json') ? asJsonValue(json['output_json']) : null,
        state: json['state'] != null ? asJsonObject(json['state']) : null,
        transitionReason: json['transition_reason'] as String?,
        prevNodeRunId: json['prev_node_run_id'] as String?,
        createdAt: json['created_at'] as String?,
        startedAt: json['started_at'] as String?,
        finishedAt: json['finished_at'] as String?,
        message: json['message'] as String?,
      );

  final String id;
  final String workflowRunId;
  final String nodeId;
  final String status;
  final int attempt;
  final JsonRecord parameters;
  final JsonValue outputJson;
  final JsonRecord? state;
  final String? transitionReason;
  final String? prevNodeRunId;
  final String? createdAt;
  final String? startedAt;
  final String? finishedAt;
  final String? message;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_run_id': workflowRunId,
        'node_id': nodeId,
        'status': status,
        'attempt': attempt,
        'parameters': parameters,
        'output_json': outputJson,
        'state': state,
        'transition_reason': transitionReason,
        'prev_node_run_id': prevNodeRunId,
        'created_at': createdAt,
        'started_at': startedAt,
        'finished_at': finishedAt,
        'message': message,
      };
}
