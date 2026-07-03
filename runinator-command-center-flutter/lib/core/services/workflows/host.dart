// port of core/services/workflows/host.ts + the `host` object built by
// core/services/workflows/index.ts's createWorkflowServices(). merged into one
// file since the ts source's split (interface vs. implementation) exists only
// to avoid a circular import that Dart doesn't have the same restriction on.

import '../../domain/json.dart';
import '../../domain/models/index.dart';
import '../../utils/status.dart' show isTerminalWorkflowRunStatus;
import '../../workflow/editor_defaults.dart' show isLockedWorkflowNode;
import '../../workflow/graph_model.dart';
import '../../workflow/workflow_helpers.dart';
import '../app_service.dart';
import 'state.dart';

typedef RunOperationFn = Future<T> Function<T>(String label, Future<T> Function() operation);

/// `Blob` (browser type) is represented as an opaque `Object` for downloadBlob,
/// same convention as core/platform.
class WorkflowServiceDeps {
  const WorkflowServiceDeps({
    required this.app,
    required this.getProviders,
    this.refreshResources,
    this.confirm,
    this.downloadBlob,
    this.downloadTextFile,
  });

  final AppNotifier app;
  final List<ProviderMetadata> Function() getProviders;
  final void Function()? refreshResources;
  final bool Function(String message)? confirm;
  final void Function(String fileName, Object blob)? downloadBlob;
  final void Function(String fileName, String contents, [String mimeType])? downloadTextFile;
}

/// wraps [WorkflowServiceHost.state] mutation + [WorkflowServiceHost.notify] so
/// catalog/editor/runs can share one live, mutable state object exactly as the
/// ts source's `host.state.field = value; host.notify();` pattern does.
class WorkflowServiceHost {
  WorkflowServiceHost({
    required WorkflowServiceDeps deps,
    required this.internal,
    required this._getState,
    required this._setState,
  }) : _rawDeps = deps;

  final WorkflowServiceDeps _rawDeps;
  final WorkflowServicesInternal internal;
  final WorkflowServicesState Function() _getState;
  final void Function(WorkflowServicesState) _setState;

  /// mirrors the ts source's `Required<WorkflowServiceDeps>` defaulting.
  bool confirm(String message) => _rawDeps.confirm?.call(message) ?? true;

  void downloadBlob(String fileName, Object blob) => _rawDeps.downloadBlob?.call(fileName, blob);

  void downloadTextFile(String fileName, String contents, [String mimeType = 'text/plain']) =>
      _rawDeps.downloadTextFile?.call(fileName, contents, mimeType);

  void refreshResourcesDep() => _rawDeps.refreshResources?.call();

  AppNotifier get app => _rawDeps.app;

  List<ProviderMetadata> Function() get getProviders => _rawDeps.getProviders;

  WorkflowServicesState get state => _getState();

  void notify() => _setState(state.clone());

  // --- ctx (mirrors host.ctx in the ts source) ---

  Future<T> runOperation<T>(String label, Future<T> Function() operation) =>
      app.runOperation(label, operation);

  void setStatus(String text) => app.setStatus(text);

  void setError(String text) => app.setError(text);

  String get normalizedSearch => app.normalizedSearch;

  // --- derived getters (ported from index.ts's createWorkflowServices) ---

  WorkflowDefinition? getSelectedWorkflow() {
    for (final workflow in state.workflows) {
      if (workflow.id == state.selectedWorkflowId) return workflow;
    }
    return null;
  }

  RuninatorType? getSelectedWorkflowInputType() {
    final workflow = getSelectedWorkflow();
    return workflow != null ? workflowInputType(workflow) : null;
  }

  bool selectedWorkflowHasInputs() {
    final ty = getSelectedWorkflowInputType();
    return ty is RuninatorTypeStruct && ty.fields.isNotEmpty;
  }

  DebugFrame? getDebugState() => DebugFrame.fromCoercedJson(state.workflowRunDetail?.run.state?['debug']);

  bool isDebugRun() => getDebugState()?.enabled == true;

  ControlFrame? getControlState() => ControlFrame.fromCoercedJson(state.workflowRunDetail?.run.state?['control']);

  bool canStepWorkflowRun() => state.workflowRunDetail?.run.status == 'debug_paused';

  bool canContinueWorkflowRun() => state.workflowRunDetail?.run.status == 'debug_paused';

  bool canPauseWorkflowRun() {
    final status = state.workflowRunDetail?.run.status;
    return status != null && ['running', 'waiting', 'approval_required'].contains(status) && getControlState()?.pauseRequested != true;
  }

  bool canResumeWorkflowRun() {
    final status = state.workflowRunDetail?.run.status;
    return status == 'paused' || (status == 'debug_paused' && getControlState()?.pauseRequested == true);
  }

  bool canCancelWorkflowRun() {
    final status = state.workflowRunDetail?.run.status;
    if (status == null) return false;
    return !['succeeded', 'failed', 'canceled', 'timed_out'].contains(status);
  }

  List<String> getCurrentBreakpoints() => getDebugState()?.breakpoints ?? [];

  bool canRemoveSelectedStep() {
    final node = asArray(state.workflowDraft.definition['nodes']).where((item) => item['id'] == state.selectedStepId);
    return node.isNotEmpty && !isLockedWorkflowNode(node.first);
  }

  List<WorkflowDefinition> getFilteredWorkflows() {
    final query = normalizedSearch;
    if (query.isEmpty) return state.workflows;
    return state.workflows
        .where((workflow) => [workflow.name, workflow.id ?? '', workflow.version].any((v) => v.toLowerCase().contains(query)))
        .toList();
  }

  List<WorkflowDefinition> getScopedWorkflows({required String scopeFilter, String? activeOrgId}) {
    final list = getFilteredWorkflows();

    if (scopeFilter == 'global') {
      return list.where((workflow) => workflow.orgId == null || workflow.orgId!.isEmpty).toList();
    }

    if (scopeFilter == 'org' && activeOrgId != null && activeOrgId.isNotEmpty) {
      return list.where((workflow) => workflow.orgId == activeOrgId).toList();
    }

    return list;
  }

  Map<String, String> getSubflowNames() => {
        for (final w in state.workflows)
          if (w.id != null) w.id!: w.name,
      };

  List<GraphNodeModel> buildDraftGraphNodes() =>
      buildGraphNodeModels(state.workflowDraft, null, subflowNames: getSubflowNames(), providers: getProviders());

  List<GraphEdgeModel> buildDraftGraphEdges() => buildGraphEdgeModels(state.workflowDraft);

  List<WorkflowValidationIssue> getGraphValidationIssues() =>
      validateWorkflowIssues(state.workflowDraft.definition, getProviders());

  WorkflowDefinition? getWorkflowRunWorkflow() {
    final snapshot = runWorkflowSnapshot(state.workflowRunDetail);
    if (snapshot != null) return snapshot;

    String? workflowId = state.workflowRunDetail?.run.workflowId;
    if (workflowId == null) {
      for (final run in state.workflowRuns) {
        if (run.id == state.selectedWorkflowRunId) {
          workflowId = run.workflowId;
          break;
        }
      }
    }

    for (final workflow in state.workflows) {
      if (workflow.id == workflowId) return workflow;
    }

    return null;
  }

  /// returns a live view onto `state.workflowDraft.definition['nodes']` (not a
  /// filtered copy like `recordArray`/`asArray`) — mirrors the ts source's raw
  /// `as JsonRecord[]` cast, since callers mutate the returned list in place.
  List<JsonRecord> ensureWorkflowNodes() {
    if (state.workflowDraft.definition['nodes'] is! List) {
      state.workflowDraft.definition['nodes'] = <Object?>[];
    }

    return (state.workflowDraft.definition['nodes'] as List).cast<JsonRecord>();
  }

  JsonRecord? getSelectedNode() {
    final match = ensureWorkflowNodes().where((item) => item['id'] == state.selectedStepId);
    return match.isNotEmpty ? match.first : null;
  }

  GraphEdgeModel? getSelectedGraphEdge() {
    final match = buildDraftGraphEdges().where((edge) => edge.id == state.selectedGraphEdgeId);
    return match.isNotEmpty ? match.first : null;
  }
}

bool isWorkflowRunTerminal(String? status) => isTerminalWorkflowRunStatus(status);

/// deep-clone helpers for typed models via a toJson/fromJson round-trip. the ts
/// source's generic `cloneJson<T>(value: T): T` (JSON.parse(JSON.stringify(value)))
/// works for any value because JS has no static type distinction; Dart's
/// `jsonDecode(jsonEncode(value)) as T` would throw at runtime for a typed model
/// (the decoded value is a plain Map, not the model class), so each typed model
/// gets its own explicit clone extension instead.
extension WorkflowDefinitionClone on WorkflowDefinition {
  WorkflowDefinition cloneDeep() => WorkflowDefinition.fromJson(toJson());
}

extension WorkflowTriggerClone on WorkflowTrigger {
  WorkflowTrigger cloneDeep() => WorkflowTrigger.fromJson(toJson());
}
