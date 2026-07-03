// port of core/domain/models/artifact/run-artifact.ts.

import '../../json.dart';

class RunArtifact {
  const RunArtifact({
    required this.id,
    required this.runId,
    this.workflowNodeRunId,
    required this.name,
    required this.mimeType,
    required this.sizeBytes,
    required this.uri,
    this.metadata,
    required this.createdAt,
  });

  factory RunArtifact.fromJson(Map<String, Object?> json) => RunArtifact(
        id: json['id'] as String,
        runId: json['run_id'] as String,
        workflowNodeRunId: json['workflow_node_run_id'] as String?,
        name: json['name'] as String,
        mimeType: json['mime_type'] as String,
        sizeBytes: (json['size_bytes'] as num).toInt(),
        uri: json['uri'] as String,
        metadata: json['metadata'] != null ? asJsonObject(json['metadata']) : null,
        createdAt: json['created_at'] as String,
      );

  final String id;
  final String runId;
  final String? workflowNodeRunId;
  final String name;
  final String mimeType;
  final int sizeBytes;
  final String uri;
  final JsonObject? metadata;
  final String createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'run_id': runId,
        'workflow_node_run_id': workflowNodeRunId,
        'name': name,
        'mime_type': mimeType,
        'size_bytes': sizeBytes,
        'uri': uri,
        'metadata': metadata,
        'created_at': createdAt,
      };
}
