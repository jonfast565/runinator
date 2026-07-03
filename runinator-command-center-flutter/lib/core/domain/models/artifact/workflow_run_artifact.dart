// port of core/domain/models/artifact/workflow-run-artifact.ts.

import '../../json.dart';

class WorkflowRunArtifact {
  const WorkflowRunArtifact({
    required this.id,
    required this.workflowRunId,
    required this.nodeId,
    required this.artifactId,
    required this.name,
    required this.mimeType,
    required this.sizeBytes,
    required this.uri,
    this.metadata,
    required this.createdAt,
  });

  factory WorkflowRunArtifact.fromJson(Map<String, Object?> json) => WorkflowRunArtifact(
        id: json['id'] as String,
        workflowRunId: json['workflow_run_id'] as String,
        nodeId: json['node_id'] as String,
        artifactId: json['artifact_id'] as String,
        name: json['name'] as String,
        mimeType: json['mime_type'] as String,
        sizeBytes: (json['size_bytes'] as num).toInt(),
        uri: json['uri'] as String,
        metadata: json['metadata'] != null ? asJsonObject(json['metadata']) : null,
        createdAt: json['created_at'] as String,
      );

  final String id;
  final String workflowRunId;
  final String nodeId;
  final String artifactId;
  final String name;
  final String mimeType;
  final int sizeBytes;
  final String uri;
  final JsonObject? metadata;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'workflow_run_id': workflowRunId,
        'node_id': nodeId,
        'artifact_id': artifactId,
        'name': name,
        'mime_type': mimeType,
        'size_bytes': sizeBytes,
        'uri': uri,
        'metadata': metadata,
        'created_at': createdAt,
      };
}
