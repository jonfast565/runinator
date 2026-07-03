// port of core/services/workflow-run-extras.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import '../platform/index.dart' show getPlatformAdapter;
import 'app_service.dart';

part 'workflow_run_extras_service.g.dart';

class WorkflowRunExtrasService {
  const WorkflowRunExtrasService(this._app);

  final AppNotifier _app;

  Future<List<RunArtifact>> fetchNodeRunArtifacts(String nodeRunId) async {
    try {
      return await _app.runOperation('Loading node run artifacts', () => api.fetchWorkflowNodeRunArtifacts(nodeRunId));
    } catch (_) {
      return [];
    }
  }

  Future<List<WorkflowRunArtifact>> fetchRunArtifacts(String runId) async {
    try {
      return await _app.runOperation('Loading run artifacts', () => api.fetchWorkflowRunArtifacts(runId));
    } catch (_) {
      return [];
    }
  }

  Future<List<RunChunk>> fetchNodeRunChunks(String nodeRunId) =>
      _app.runOperation('Loading node run log', () => api.fetchWorkflowNodeRunChunks(nodeRunId));

  Future<void> downloadArtifact(String artifactId, String name) => _app.runOperation<Object?>('Downloading $name', () async {
        final artifacts = getPlatformAdapter().artifacts;

        if (artifacts.isDesktop()) {
          return artifacts.downloadToPath(artifactId, name);
        }

        await artifacts.downloadInBrowser(artifactId, name);
        return null;
      });

  Future<TaskResponse> deliverSignal(String workflowRunId, String name, [Object? payload = const {}]) =>
      _app.runOperation("Sending signal '$name'", () => api.deliverSignal(workflowRunId, name, payload));

  Future<TaskResponse> resolveInput(
    String nodeRunId,
    Object? outputJson, {
    String? resolvedBy,
    String? message,
  }) =>
      _app.runOperation(
        'Resolving workflow input',
        () => api.resolveWorkflowInput(nodeRunId, outputJson, resolvedBy: resolvedBy, message: message),
      );
}

@riverpod
WorkflowRunExtrasService workflowRunExtrasService(Ref ref) =>
    WorkflowRunExtrasService(ref.watch(appProvider.notifier));
