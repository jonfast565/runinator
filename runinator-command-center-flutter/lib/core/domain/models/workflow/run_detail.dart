// port of core/domain/models/workflow/run-detail.ts.

import '../../json.dart';
import '../run/run_summary.dart';
import 'definition.dart';
import 'node_run.dart';

/// RunSummary with workflow_id/workflow_snapshot/message required, matching the ts
/// source's `RunSummary & { workflow_id: string; ... }` intersection type.
class WorkflowRunDetailRun {
  const WorkflowRunDetailRun({
    required this.id,
    required this.workflowId,
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

  factory WorkflowRunDetailRun.fromJson(Map<String, Object?> json) => WorkflowRunDetailRun(
        id: json['id'] as String,
        workflowId: json['workflow_id'] as String,
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
  final String workflowId;
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

  RunSummary toRunSummary() => RunSummary(
        id: id,
        workflowId: workflowId,
        workflowSnapshot: workflowSnapshot,
        status: status,
        parameters: parameters,
        outputJson: outputJson,
        message: message,
        trigger: trigger,
        createdAt: createdAt,
        startedAt: startedAt,
        finishedAt: finishedAt,
        workflowRunId: workflowRunId,
        workflowNodeId: workflowNodeId,
        activeNodeId: activeNodeId,
        state: state,
        name: name,
      );

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

class WorkflowRunDetail {
  const WorkflowRunDetail({required this.run, required this.nodes});

  factory WorkflowRunDetail.fromJson(Map<String, Object?> json) => WorkflowRunDetail(
        run: WorkflowRunDetailRun.fromJson(json['run'] as Map<String, Object?>),
        nodes: (json['nodes'] as List)
            .map((n) => WorkflowNodeRun.fromJson(n as Map<String, Object?>))
            .toList(),
      );

  final WorkflowRunDetailRun run;
  final List<WorkflowNodeRun> nodes;

  Map<String, Object?> toJson() => {
        'run': run.toJson(),
        'nodes': nodes.map((n) => n.toJson()).toList(),
      };
}

/// snapshot attached to a run detail, when the backend included the workflow definition.
WorkflowDefinition? runWorkflowSnapshot(WorkflowRunDetail? detail) {
  final snapshot = detail?.run.workflowSnapshot;
  return snapshot != null ? WorkflowDefinition.fromJson(snapshot) : null;
}
