// port of core/services/workflows/runs.ts. the hardest single file in the port:
// stale-response fencing (applyWorkflowRunDetail), run-tab LRU eviction
// (MAX_OPEN_RUN_TABS), and optimistic breakpoint toggle-with-rollback are ported
// verbatim, not just their shape, since they are load-bearing correctness logic.

import 'dart:convert';

import '../../api/command_center_api.dart' as api;
import '../../domain/json.dart';
import '../../domain/models/index.dart';
import '../../navigation/app_tab.dart';
import '../../utils/json_utils.dart' show parseObject;
import '../../workflow/editor_defaults.dart' show buildInputSkeleton;
import '../../workflow/workflow_helpers.dart' show nodeRef, nodeRefId;
import 'catalog.dart' show WorkflowRunsPeer;
import 'editor.dart' show WorkflowRunsEditorPeer;
import 'host.dart';
import 'state.dart';

const int _maxOpenRunTabs = 8;
const String _watchStoragePrefix = 'runinator.watch.';

/// core/ has no browser localStorage dependency; a concrete web platform
/// adapter (future UI pass) supplies one via [setWatchExpressionStorage].
WatchExpressionStorage? _watchStorage;

void setWatchExpressionStorage(WatchExpressionStorage storage) {
  _watchStorage = storage;
}

abstract class WatchExpressionStorage {
  int get length;

  String? keyAt(int index);

  String? getItem(String key);

  void setItem(String key, String value);
}

class WorkflowRunDetailMetadata {
  const WorkflowRunDetailMetadata.http({required this.requestStartedVersion, required this.requestId}) : source = 'http';

  const WorkflowRunDetailMetadata.ws()
      : source = 'ws',
        requestStartedVersion = null,
        requestId = null;

  final String source;
  final int? requestStartedVersion;
  final int? requestId;
}

class WorkflowRunService implements WorkflowRunsPeer, WorkflowRunsEditorPeer {
  WorkflowRunService(this._host);

  final WorkflowServiceHost _host;

  bool isBreakpointed(String nodeId) => _host.getCurrentBreakpoints().contains(nodeId);

  String getTransition(String key) {
    final transitions = parseObject(_host.state.stepEditor.transitionsJson, {});
    return nodeRefId(transitions[key]) ?? '';
  }

  void setTransition(String key, String value) {
    final transitions = parseObject(_host.state.stepEditor.transitionsJson, {});

    if (value.isNotEmpty) {
      transitions[key] = nodeRef(value);
    } else {
      _host.state.stepEditor.transitionsJson = const JsonEncoder.withIndent('  ').convert({
        for (final entry in transitions.entries)
          if (entry.key != key) entry.key: entry.value,
      });
      _host.state.isDirty = true;
      return;
    }

    _host.state.stepEditor.transitionsJson = const JsonEncoder.withIndent('  ').convert(transitions);
    _host.state.isDirty = true;
    _host.notify();
  }

  Future<void> runSelectedWorkflow([bool debug = false]) async {
    final workflow = _host.getSelectedWorkflow();

    if (workflow?.id == null || !workflow!.enabled) {
      _host.setError(workflow != null ? 'Workflow is disabled' : 'No workflow selected');
      return;
    }

    if (_host.selectedWorkflowHasInputs()) {
      _host.state.runInputDraft = buildInputSkeleton(_host.getSelectedWorkflowInputType());
      _host.state.runInputDebug = debug;
      _host.state.runInputOpen = true;
      return;
    }

    await launchWorkflowRun(debug, {});
    _host.notify();
  }

  Future<void> runSelectedWorkflowDebug() => runSelectedWorkflow(true);

  void closeRunInput() {
    _host.state.runInputOpen = false;
    _host.notify();
  }

  Future<void> confirmRunInput() async {
    final debug = _host.state.runInputDebug;
    final parameters = _host.state.runInputDraft;
    _host.state.runInputOpen = false;
    await launchWorkflowRun(debug, parameters);
    _host.notify();
  }

  Future<void> launchWorkflowRun(bool debug, JsonRecord parameters) async {
    final workflow = _host.getSelectedWorkflow();
    final workflowId = workflow?.id;

    if (workflowId == null || !workflow!.enabled) {
      _host.setError(workflow != null ? 'Workflow is disabled' : 'No workflow selected');
      return;
    }

    final response = await _host.runOperation(
      debug ? 'Running workflow ${workflow.name} in debug mode' : 'Running workflow ${workflow.name}',
      () => api.createWorkflowRun(workflowId, debug: debug, parameters: parameters),
    );
    _host.state.selectedWorkflowRunId = response.id;
    _host.setStatus('${debug ? 'Debug workflow run' : 'Workflow run'} queued: ${response.id}');
    await fetchWorkflowRunDetail(response.id);
    await fetchRecentWorkflowRuns();
    _host.app.setActiveTab(AppTab.runs);
    _host.notify();
  }

  Future<void> stepSelectedWorkflowRun() async {
    if (_host.state.workflowRunDetail == null || !_host.canStepWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Stepping workflow run $runId', () => api.stepWorkflowRun(runId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to step workflow run');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow run $runId stepped');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> continueSelectedWorkflowRun() async {
    if (_host.state.workflowRunDetail == null || !_host.canContinueWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Continuing workflow run $runId', () => api.continueWorkflowRun(runId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to continue workflow run');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow run $runId continued');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> cancelSelectedWorkflowRun() async {
    if (_host.state.workflowRunDetail == null || !_host.canCancelWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Canceling workflow run $runId', () => api.cancelWorkflowRun(runId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to cancel workflow run');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow run $runId canceled');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> pauseSelectedWorkflowRun() async {
    if (_host.state.workflowRunDetail == null || !_host.canPauseWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Pausing workflow run $runId', () => api.pauseWorkflowRun(runId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to pause workflow run');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow run $runId pause requested');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> resumeSelectedWorkflowRun() async {
    if (_host.state.workflowRunDetail == null || !_host.canResumeWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Resuming workflow run $runId', () => api.resumeWorkflowRun(runId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to resume workflow run');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow run $runId resumed');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> patchSelectedWorkflowRunDebug(api.WorkflowDebugPatch patch) async {
    if (_host.state.workflowRunDetail == null || !_host.isDebugRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Updating debug settings for run $runId', () => api.patchWorkflowRunDebug(runId, patch));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to update debug settings');
      return;
    }

    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> toggleBreakpoint(String nodeId) async {
    if (_host.state.workflowRunDetail == null || !_host.isDebugRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final current = _host.getCurrentBreakpoints();
    final next = current.contains(nodeId) ? current.where((id) => id != nodeId).toList() : [...current, nodeId];
    final mutationId = ++_host.internal.nextBreakpointMutationId;
    _host.internal.pendingBreakpointPatch = PendingBreakpointPatch(runId: runId, breakpoints: next, mutationId: mutationId);
    _applyBreakpointPatch(_host.state.workflowRunDetail, next);

    try {
      final response = await _host.runOperation(
        'Updating debug settings for run $runId',
        () => api.patchWorkflowRunDebug(runId, api.WorkflowDebugPatch(breakpoints: next)),
      );

      if (!response.success) {
        _host.setError(response.message.isNotEmpty ? response.message : 'Failed to update debug settings');

        if (_clearPendingBreakpointPatch(runId, mutationId)) {
          _applyBreakpointPatch(_host.state.workflowRunDetail, current);
        }

        return;
      }

      await fetchWorkflowRunDetail(runId, silent: true);
    } catch (_) {
      if (_clearPendingBreakpointPatch(runId, mutationId)) {
        _applyBreakpointPatch(_host.state.workflowRunDetail, current);
      }
    }
    _host.notify();
  }

  Future<void> runToCursor(String nodeId) async {
    if (_host.state.workflowRunDetail == null || !_host.isDebugRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Running to cursor $nodeId', () => api.runToCursorWorkflowRun(runId, nodeId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to run to cursor');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Running to $nodeId');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> skipCurrentNode(Object? outputJson, [String? message]) async {
    if (_host.state.workflowRunDetail == null || !_host.canStepWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Skipping current node', () => api.skipWorkflowNode(runId, outputJson, message));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to skip node');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Node skipped');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<void> rerunCurrentNode(Object? parameters) async {
    if (_host.state.workflowRunDetail == null || !_host.canStepWorkflowRun()) {
      return;
    }

    final runId = _host.state.workflowRunDetail!.run.id;
    final response = await _host.runOperation('Re-running current node', () => api.rerunWorkflowNode(runId, parameters));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to re-run node');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Node re-running');
    await fetchWorkflowRunDetail(runId, silent: true);
    _host.notify();
  }

  Future<String?> replaySelectedWorkflowRun({String? runId, String? fromStepId}) async {
    final targetId = runId ?? _host.state.workflowRunDetail?.run.id;

    if (targetId == null) {
      return null;
    }

    final label = fromStepId != null ? 'Replaying workflow run $targetId from step $fromStepId' : 'Replaying workflow run $targetId';
    WorkflowRunCreated? created;
    try {
      created = await _host.runOperation(label, () => api.replayWorkflowRun(targetId, fromStepId: fromStepId));
    } catch (error) {
      _host.setError(error.toString());
      created = null;
    }

    if (created?.id == null) {
      _host.setError('Failed to start replay');
      return null;
    }

    _host.setStatus('Replay started as run ${created!.id}');
    openRunInTab(created.id);
    activateRunTab(created.id);
    await fetchWorkflowRunDetail(created.id);
    await fetchRecentWorkflowRuns();
    _host.app.setActiveTab(AppTab.runs);
    return created.id;
  }

  Future<void> renameSelectedWorkflowRun(String runId, String? name) async {
    if (runId.isEmpty) {
      return;
    }

    TaskResponse? response;
    try {
      response = await _host.runOperation('Renaming run $runId', () => api.renameWorkflowRun(runId, name));
    } catch (error) {
      _host.setError(error.toString());
      response = null;
    }

    if (response == null) {
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Run renamed');
    await fetchRecentWorkflowRuns();

    if (_host.state.workflowRunDetail?.run.id == runId) {
      await fetchWorkflowRunDetail(runId, silent: true);
    }
    _host.notify();
  }

  Map<String, List<String>> loadAllWatchExpressions() {
    final storage = _watchStorage;

    if (storage == null) {
      return {};
    }

    final result = <String, List<String>>{};

    for (var i = 0; i < storage.length; i++) {
      final key = storage.keyAt(i);

      if (key == null || !key.startsWith(_watchStoragePrefix)) {
        continue;
      }

      final id = key.substring(_watchStoragePrefix.length);

      if (id.isEmpty) {
        continue;
      }

      try {
        final parsed = jsonDecode(storage.getItem(key) ?? '[]');

        if (parsed is List) {
          result[id] = parsed.whereType<String>().toList();
        }
      } catch (_) {
        // ignore
      }
    }

    return result;
  }

  void persistWatchExpressions(String workflowId, List<String> list) {
    _watchStorage?.setItem('$_watchStoragePrefix$workflowId', jsonEncode(list));
  }

  void addWatchExpression(String expression) {
    final workflowId = _host.getWorkflowRunWorkflow()?.id;

    if (workflowId == null || expression.trim().isEmpty) {
      return;
    }

    final existing = _host.state.watchExpressionsByWorkflowId[workflowId] ?? [];

    if (existing.contains(expression)) {
      return;
    }

    final next = [...existing, expression];
    _host.state.watchExpressionsByWorkflowId = {..._host.state.watchExpressionsByWorkflowId, workflowId: next};
    persistWatchExpressions(workflowId, next);
    _host.notify();
  }

  void removeWatchExpression(String expression) {
    final workflowId = _host.getWorkflowRunWorkflow()?.id;

    if (workflowId == null) {
      return;
    }

    final existing = _host.state.watchExpressionsByWorkflowId[workflowId] ?? [];
    final next = existing.where((e) => e != expression).toList();
    _host.state.watchExpressionsByWorkflowId = {..._host.state.watchExpressionsByWorkflowId, workflowId: next};
    persistWatchExpressions(workflowId, next);
    _host.notify();
  }

  Future<void> fetchWorkflowRunsForSelected(String workflowId) async {
    List<RunSummary> runs;
    try {
      runs = await _host.runOperation('Loading workflow runs', () => api.fetchWorkflowRuns(workflowId));
    } catch (_) {
      runs = [];
    }
    _host.state.workflowRuns = runs;

    if (!_host.state.workflowRuns.any((run) => run.id == _host.state.selectedWorkflowRunId)) {
      _host.state.selectedWorkflowRunId = _host.state.workflowRuns.isNotEmpty ? _host.state.workflowRuns.first.id : null;
    }
    _host.notify();
  }

  Future<void> fetchRecentWorkflowRuns() async {
    List<RunSummary> runs;
    try {
      runs = await _host.runOperation('Loading workflow runs', () => api.fetchWorkflowRuns());
    } catch (_) {
      runs = [];
    }
    _host.state.workflowRuns = runs;
    final previousRunId = _host.state.selectedWorkflowRunId;

    if (_host.state.selectedWorkflowRunId == null && _host.state.workflowRuns.isNotEmpty) {
      final first = _host.state.workflowRuns.first.id;
      openRunInTab(first);
      activateRunTab(first);
    }

    final currentRunId = _host.state.selectedWorkflowRunId;

    if (currentRunId != null && (_host.state.workflowRunDetail == null || previousRunId != currentRunId)) {
      await fetchWorkflowRunDetail(currentRunId, silent: true);
    }
    _host.notify();
  }

  Future<void> selectWorkflowRun(RunSummary run) async {
    openRunInTab(run.id);
    activateRunTab(run.id);
    return fetchWorkflowRunDetail(run.id);
  }

  void openRunInTab(String runId) {
    if (runId.isEmpty) {
      return;
    }

    final ids = _host.state.openRunIds;

    if (!ids.contains(runId)) {
      // cap the tab count by evicting the oldest non-active tab.
      if (ids.length >= _maxOpenRunTabs) {
        final victimMatch = ids.where((id) => id != _host.state.selectedWorkflowRunId);

        if (victimMatch.isNotEmpty) {
          closeRunTab(victimMatch.first);
        }
      }

      _host.state.openRunIds = [..._host.state.openRunIds, runId];
    }

    if (!_host.internal.runDetailById.containsKey(runId)) {
      _host.internal.runDetailById[runId] = null;
    }
    _host.notify();
  }

  void activateRunTab(String runId) {
    if (runId.isEmpty) {
      return;
    }

    if (!_host.state.openRunIds.contains(runId)) {
      openRunInTab(runId);
    }

    _host.state.selectedWorkflowRunId = runId;
    final tabDetail = _host.internal.runDetailById[runId];
    _host.state.workflowRunDetail = tabDetail;
    _host.state.workflowNodeDetailExtra = '';
    _host.state.selectedWorkflowRunNodeId = (tabDetail != null && tabDetail.nodes.isNotEmpty) ? tabDetail.nodes.first.nodeId : '';

    if (tabDetail != null) {
      syncWorkflowRunGatesForDetail(tabDetail);
    } else {
      clearWorkflowRunGates();
    }

    if (_host.internal.runDetailById[runId] == null) {
      fetchWorkflowRunDetail(runId, silent: true);
    }
    _host.notify();
  }

  void closeRunTab(String runId) {
    final ids = _host.state.openRunIds;
    final index = ids.indexOf(runId);

    if (index == -1) {
      return;
    }

    final next = [...ids.sublist(0, index), ...ids.sublist(index + 1)];
    _host.state.openRunIds = next;
    _host.internal.runDetailById.remove(runId);
    _host.internal.latestWorkflowRunPushVersion.remove(runId);
    _host.internal.latestWorkflowRunHttpRequest.remove(runId);

    if (_host.state.selectedWorkflowRunId == runId) {
      final replacement = next.isNotEmpty ? next[index.clamp(0, next.length - 1)] : null;

      if (replacement != null) {
        activateRunTab(replacement);
      } else {
        _host.state.selectedWorkflowRunId = null;
        _host.state.workflowRunDetail = null;
        _host.state.selectedWorkflowRunNodeId = '';
        clearWorkflowRunGates();
      }
    }
    _host.notify();
  }

  Future<void> fetchWorkflowRunDetail(String workflowRunId, {bool silent = false}) async {
    final requestStartedVersion = ++_host.internal.nextWorkflowRunDetailVersion;
    final requestId = ++_host.internal.nextWorkflowRunHttpRequestId;
    _host.internal.latestWorkflowRunHttpRequest[workflowRunId] = requestId;

    WorkflowRunDetail? detail;
    try {
      detail = silent
          ? await api.fetchWorkflowRun(workflowRunId)
          : await _host.runOperation('Loading workflow run', () => api.fetchWorkflowRun(workflowRunId));
    } catch (_) {
      detail = null;
    }

    applyWorkflowRunDetail(
      detail,
      WorkflowRunDetailMetadata.http(requestStartedVersion: requestStartedVersion, requestId: requestId),
    );
  }

  void setWorkflowRunDetail(WorkflowRunDetail? detail) {
    if (detail != null) {
      _host.internal.latestWorkflowRunPushVersion[detail.run.id] = ++_host.internal.nextWorkflowRunDetailVersion;
    }

    applyWorkflowRunDetail(detail, const WorkflowRunDetailMetadata.ws());
  }

  void selectWorkflowRunNode(String nodeId) {
    _host.state.selectedWorkflowRunNodeId = nodeId;
    updateSelectedWorkflowNodeDetail();
    _host.notify();
  }

  @override
  void clearWorkflowRunGates() {
    _host.state.workflowRunGates = [];
    _host.state.workflowRunGateRunId = null;
    _host.state.workflowRunGateFingerprint = '';
    _host.notify();
  }

  List<String> workflowRunGateIds(WorkflowRunDetail? detail) {
    if (detail == null) {
      return [];
    }

    final ids = detail.nodes
        .map((node) => node.state?['gate_id'])
        .where((value) => value is String && value.isNotEmpty)
        .cast<String>()
        .toSet()
        .toList()
      ..sort();
    return ids;
  }

  String workflowRunGateFingerprintForDetail(WorkflowRunDetail? detail) => workflowRunGateIds(detail).join(',');

  Future<void> refreshWorkflowRunGates(String runId, [bool force = false]) async {
    final activeDetail = runId == _host.state.workflowRunDetail?.run.id ? _host.state.workflowRunDetail : _host.internal.runDetailById[runId];
    final fingerprint = workflowRunGateFingerprintForDetail(activeDetail);

    if (!force && _host.state.workflowRunGateRunId == runId && _host.state.workflowRunGateFingerprint == fingerprint) {
      return;
    }

    final requestId = ++_host.internal.nextWorkflowRunGateRequestId;
    List<GateRecord>? gates;
    try {
      gates = await api.fetchGates(workflowRunId: runId);
    } catch (_) {
      gates = null;
    }

    if (requestId != _host.internal.nextWorkflowRunGateRequestId) {
      return;
    }

    if (_host.state.selectedWorkflowRunId != runId && _host.state.workflowRunDetail?.run.id != runId) {
      return;
    }

    _host.state.workflowRunGates = gates ?? [];
    _host.state.workflowRunGateRunId = runId;
    _host.state.workflowRunGateFingerprint = fingerprint;
    _host.notify();
  }

  Future<void> syncWorkflowRunGatesForDetail(WorkflowRunDetail? detail, [bool force = false]) async {
    if (detail == null) {
      clearWorkflowRunGates();
      return;
    }

    await refreshWorkflowRunGates(detail.run.id, force);
  }

  Future<void> resolveWorkflowRunGate(String gateId, String action, [String? reason]) async {
    final runId = _host.state.workflowRunDetail?.run.id ?? _host.state.selectedWorkflowRunId;

    if (runId == null) {
      _host.setError('No workflow run selected');
      return;
    }

    final trimmed = (reason != null && reason.trim().isNotEmpty) ? reason.trim() : null;
    final response = await _host.runOperation(
      action == 'open' ? 'Opening gate' : 'Closing gate',
      () => action == 'open' ? api.openGate(gateId, trimmed) : api.closeGate(gateId, trimmed),
    );
    _host.setStatus(response.message.isNotEmpty ? response.message : 'Gate ${action == 'open' ? 'opened' : 'closed'}');
    await Future.wait([fetchWorkflowRunDetail(runId, silent: true), refreshWorkflowRunGates(runId, true)]);
    _host.notify();
  }

  void applyWorkflowRunDetail(WorkflowRunDetail? detail, WorkflowRunDetailMetadata metadata) {
    if (detail != null && metadata.source == 'http') {
      final latestPushVersion = _host.internal.latestWorkflowRunPushVersion[detail.run.id] ?? 0;
      final latestRequestId = _host.internal.latestWorkflowRunHttpRequest[detail.run.id] ?? 0;

      if (latestPushVersion > metadata.requestStartedVersion! || latestRequestId != metadata.requestId) {
        // dropped stale workflow run detail: a WS push (or a newer HTTP request)
        // for this run already landed after this request started.
        return;
      }
    }

    if (detail != null) {
      _confirmPendingBreakpointPatch(detail);
    }

    if (detail != null) {
      _host.internal.runDetailById[detail.run.id] = detail;

      if (!_host.state.openRunIds.contains(detail.run.id)) {
        final next = [..._host.state.openRunIds, detail.run.id];
        _host.state.openRunIds = next.length > _maxOpenRunTabs ? next.sublist(next.length - _maxOpenRunTabs) : next;
      }

      _host.state.selectedWorkflowRunId ??= detail.run.id;
    }

    final isActiveRun = detail != null ? detail.run.id == _host.state.selectedWorkflowRunId : true;

    if (isActiveRun) {
      _host.state.workflowRunDetail = detail;
      _reapplyPendingBreakpointPatch();
      _host.state.workflowNodeDetailExtra = '';

      if (detail == null || !detail.nodes.any((node) => node.nodeId == _host.state.selectedWorkflowRunNodeId)) {
        _host.state.selectedWorkflowRunNodeId = (detail != null && detail.nodes.isNotEmpty) ? detail.nodes.first.nodeId : '';
      }

      if (detail != null) {
        syncWorkflowRunGatesForDetail(detail);
      } else {
        clearWorkflowRunGates();
      }
    }

    if (detail != null) {
      final hasWaiting = detail.nodes.any((n) => n.status == 'waiting' || n.status == 'approval_required' || n.status == 'pending');
      if (hasWaiting) {
        _host.refreshResourcesDep();
      }
    }
    _host.notify();
  }

  void _reapplyPendingBreakpointPatch() {
    final pending = _host.internal.pendingBreakpointPatch;
    if (_host.state.workflowRunDetail == null || pending == null) {
      return;
    }

    if (_host.state.workflowRunDetail!.run.id != pending.runId) {
      return;
    }

    _applyBreakpointPatch(_host.state.workflowRunDetail, pending.breakpoints);
    _host.notify();
  }

  void _confirmPendingBreakpointPatch(WorkflowRunDetail detail) {
    final pending = _host.internal.pendingBreakpointPatch;

    if (pending?.runId != detail.run.id) {
      return;
    }

    if (_sameBreakpoints(_readBreakpoints(detail), pending!.breakpoints)) {
      _host.internal.pendingBreakpointPatch = null;
    }
  }

  bool _clearPendingBreakpointPatch(String runId, int mutationId) {
    final pending = _host.internal.pendingBreakpointPatch;

    if (pending?.runId == runId && pending?.mutationId == mutationId) {
      _host.internal.pendingBreakpointPatch = null;
      return true;
    }

    return false;
  }

  void _applyBreakpointPatch(WorkflowRunDetail? detail, List<String> breakpoints) {
    // mutates the run's `state.debug.breakpoints` in place, mirroring the ts
    // source's optimistic-update pattern (the run detail's `state` JsonRecord is
    // reassigned with the merged debug frame).
    if (detail == null || detail.run.state == null) {
      return;
    }

    final debug = DebugFrame.fromCoercedJson(detail.run.state?['debug']);
    final nextState = <String, Object?>{...(detail.run.state ?? {})};
    nextState['debug'] = <String, Object?>{
      ...?debug?.toJson(),
      'breakpoints': [...breakpoints],
    };

    final run = detail.run;
    final newRun = WorkflowRunDetailRun(
      id: run.id,
      workflowId: run.workflowId,
      workflowSnapshot: run.workflowSnapshot,
      status: run.status,
      parameters: run.parameters,
      outputJson: run.outputJson,
      message: run.message,
      trigger: run.trigger,
      createdAt: run.createdAt,
      startedAt: run.startedAt,
      finishedAt: run.finishedAt,
      workflowRunId: run.workflowRunId,
      workflowNodeId: run.workflowNodeId,
      activeNodeId: run.activeNodeId,
      state: nextState,
      name: run.name,
    );

    // replace the run detail's run in place via the same map identity held by
    // _host.internal.runDetailById / _host.state.workflowRunDetail.
    _host.internal.runDetailById[detail.run.id] = WorkflowRunDetail(run: newRun, nodes: detail.nodes);

    if (_host.state.workflowRunDetail?.run.id == detail.run.id) {
      _host.state.workflowRunDetail = WorkflowRunDetail(run: newRun, nodes: detail.nodes);
    }
  }

  List<String> _readBreakpoints(WorkflowRunDetail detail) => DebugFrame.fromCoercedJson(detail.run.state?['debug'])?.breakpoints ?? [];

  bool _sameBreakpoints(List<String> left, List<String> right) {
    final normalizedLeft = left.toSet().toList()..sort();
    final normalizedRight = right.toSet().toList()..sort();

    if (normalizedLeft.length != normalizedRight.length) {
      return false;
    }

    for (var i = 0; i < normalizedLeft.length; i++) {
      if (normalizedLeft[i] != normalizedRight[i]) {
        return false;
      }
    }

    return true;
  }

  @override
  Future<void> updateSelectedWorkflowNodeDetail() async {
    _host.state.selectedWorkflowNodeRunId = null;
    _host.state.workflowNodeDetailExtra = '';
    final nodeId = _host.state.selectedWorkflowRunNodeId.isNotEmpty ? _host.state.selectedWorkflowRunNodeId : _host.state.selectedStepId;
    WorkflowNodeRun? step;
    for (final node in _host.state.workflowRunDetail?.nodes ?? const <WorkflowNodeRun>[]) {
      if (node.nodeId == nodeId) {
        step = node;
        break;
      }
    }

    if (step == null) {
      return;
    }

    _host.state.selectedWorkflowNodeRunId = step.id;
    final results = await Future.wait([
      _host.runOperation('Loading node chunks', () => api.fetchWorkflowNodeRunChunks(step!.id)).catchError((_) => <RunChunk>[]),
      _host.runOperation('Loading node artifacts', () => api.fetchWorkflowNodeRunArtifacts(step!.id)).catchError((_) => <RunArtifact>[]),
    ]);
    final nodeChunks = results[0] as List<RunChunk>;
    final nodeArtifacts = results[1] as List<RunArtifact>;
    _host.state.workflowNodeDetailExtra = [
      '',
      'Workflow node run ${step.id} chunks',
      ...nodeChunks.map((chunk) => '[${chunk.stream}] ${chunk.content}'),
      '',
      'Workflow node run ${step.id} artifacts',
      ...nodeArtifacts.map((artifact) => '${artifact.name} (${artifact.sizeBytes} bytes) ${artifact.uri}'),
    ].join('\n');
    _host.notify();
  }
}
