import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';

part 'dev_pack_service.g.dart';

class DevPackService {
  const DevPackService(this._app);

  final AppNotifier _app;

  Future<DevPackInspectResult> inspect(String path, [bool skipSettings = false]) =>
      _app.runOperation('Inspecting dev pack', () => api.inspectDevPack(path, skipSettings));

  Future<DevPackTextFile> readFile(String path) =>
      _app.runOperation('Reading dev pack file', () => api.readDevPackFile(path));

  Future<DevPackTextFile> writeFile(String path, String contents) =>
      _app.runOperation('Writing dev pack file', () => api.writeDevPackFile(path, contents));

  Future<DevPackApplyResult> apply(String path, [bool skipSettings = false]) =>
      _app.runOperation('Applying dev pack', () => api.applyDevPack(path, skipSettings));

  Future<WorkflowRunCreated> createRun(String workflowId, {bool debug = false, Object? parameters}) =>
      _app.runOperation(
        'Starting workflow run',
        () => api.createWorkflowRun(workflowId, debug: debug, parameters: parameters),
      );

  Future<WorkflowRunDetail> fetchRun(String runId) =>
      _app.runOperation('Loading workflow run', () => api.fetchWorkflowRun(runId));

  Future<TaskResponse> cancelRun(String runId) =>
      _app.runOperation('Canceling workflow run', () => api.cancelWorkflowRun(runId));

  Future<WorkflowRunCreated> replayRun(String workflowRunId, {String? fromStepId}) =>
      _app.runOperation(
        'Replaying workflow run',
        () => api.replayWorkflowRun(workflowRunId, fromStepId: fromStepId),
      );
}

@riverpod
DevPackService devPackService(Ref ref) => DevPackService(ref.watch(appProvider.notifier));
