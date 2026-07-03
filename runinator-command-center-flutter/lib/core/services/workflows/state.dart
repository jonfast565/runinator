// port of core/services/workflows/state.ts.
//
// unlike the other services (immutable state + copyWith), the workflows/*
// subsystem shares one large mutable "host" object across catalog/editor/runs,
// exactly as the ts source's `host.state.field = value` mutation style does.
// WorkflowServicesState is therefore a genuinely mutable class; `notify()`
// (in host.dart) reassigns the riverpod notifier's `state` to a shallow clone
// to trigger listeners, mirroring the source's `store.setState(c => ({...c}))`.

import 'dart:async';

import '../../domain/json.dart';
import '../../domain/models/index.dart';
import '../../utils/format.dart' show pretty;
import '../../workflow/editor_defaults.dart';
import '../../workflow/workflow_helpers.dart' show valueRef;

class ConditionBranchDraft {
  ConditionBranchDraft({required this.whenJson, required this.target});

  String whenJson;
  String target;
}

class PercentageBucketDraft {
  PercentageBucketDraft({required this.weight, required this.target});

  num weight;
  String target;
}

class AssertAssertionDraft {
  AssertAssertionDraft({required this.name, required this.conditionJson, required this.message});

  String name;
  String conditionJson;
  String message;
}

/// mirrors the ts source's `ReturnType<typeof createStepEditorState>` — a big
/// loosely-typed record of editor-modal fields, one group per node kind.
class StepEditorState {
  StepEditorState({
    this.id = '',
    this.name = '',
    this.kind = 'action',
    this.approvalType = 'generic',
    this.approvalPrompt = 'Approval required',
    this.gateKind = 'manual',
    this.gateWhenJson = '{}',
    this.gatePollInterval = 30,
    this.gateTimeout = 0,
    this.gateLabel = '',
    this.signalName = 'signal',
    this.conditionFallback = '',
    List<ConditionBranchDraft>? conditionBranches,
    this.waitSeconds = 60,
    this.waitInitialStatus = 'waiting',
    this.waitUntilStatus = '',
    this.waitJson = '{}',
    this.loopItemsJson = '[]',
    this.loopTarget = '',
    this.loopMaxIterations = 10,
    required this.switchValueJson,
    List<SwitchCaseEditor>? switchCases,
    this.switchDefault = '',
    required this.toggleValueJson,
    this.toggleOn = '',
    this.toggleOff = '',
    required this.percentageKeyJson,
    List<PercentageBucketDraft>? percentageBuckets,
    this.percentageDefault = '',
    List<String>? parallelBranches,
    List<String>? joinWaitFor,
    this.joinMode = 'all',
    this.tryBody = '',
    this.tryCatch = '',
    this.tryFinally = '',
    this.mapItemsJson = '[]',
    this.mapTarget = '',
    this.mapConcurrency = 1,
    List<String>? raceBranches,
    this.raceWinner = 'first_success',
    this.outputEventType = 'workflow.output',
    this.outputDataJson = '{}',
    this.inputPrompt = 'Provide input',
    this.configNameJson = '""',
    this.configMetadataJson = '{}',
    this.subflowId = '',
    this.subflowParametersJson = '{}',
    List<AssertAssertionDraft>? assertAssertions,
    this.transformBindingsJson = '{}',
    required this.auditActionJson,
    this.auditActorJson = '',
    this.auditTargetJson = '',
    this.auditReasonJson = '',
    this.checkpointName = '',
    this.mutexName = '',
    this.mutexPollInterval = 30,
    this.throttleName = '',
    this.throttleMaxPerWindow = 10,
    this.throttleWindowSeconds = 60,
    this.throttlePollInterval = 30,
    required this.awaitRunIdsJson,
    this.awaitMode = 'all',
    this.awaitPollInterval = 30,
    this.debounceName = '',
    this.debounceDelaySeconds = 30,
    this.debounceTriggerKeyJson = '',
    this.collectName = '',
    this.collectMax = 10,
    this.barrierName = '',
    this.barrierCount = 2,
    this.barrierPollInterval = 30,
    this.circuitName = '',
    this.circuitThreshold = 5,
    this.circuitWindowSeconds = 60,
    this.circuitCooldownSeconds = 60,
    this.eventSourceType = '*',
    this.eventSourceFilterJson = '',
    this.eventSourceMax = 0,
    this.locked = false,
    this.skipped = false,
    this.maxAttempts = 1,
    this.timeoutSeconds = 0,
    this.actionName = '',
    this.actionFunction = '',
    this.parametersJson = '{}',
    this.transitionsJson = '{}',
  })  : conditionBranches = conditionBranches ?? [],
        switchCases = switchCases ?? [],
        percentageBuckets = percentageBuckets ?? [],
        parallelBranches = parallelBranches ?? [],
        joinWaitFor = joinWaitFor ?? [],
        raceBranches = raceBranches ?? [],
        assertAssertions = assertAssertions ?? [];

  String id;
  String name;
  String kind;
  String approvalType;
  String approvalPrompt;
  String gateKind;
  String gateWhenJson;
  num gatePollInterval;
  num gateTimeout;
  String gateLabel;
  String signalName;
  String conditionFallback;
  List<ConditionBranchDraft> conditionBranches;
  num waitSeconds;
  String waitInitialStatus;
  String waitUntilStatus;
  String waitJson;
  String loopItemsJson;
  String loopTarget;
  num loopMaxIterations;
  String switchValueJson;
  List<SwitchCaseEditor> switchCases;
  String switchDefault;
  String toggleValueJson;
  String toggleOn;
  String toggleOff;
  String percentageKeyJson;
  List<PercentageBucketDraft> percentageBuckets;
  String percentageDefault;
  List<String> parallelBranches;
  List<String> joinWaitFor;
  String joinMode;
  String tryBody;
  String tryCatch;
  String tryFinally;
  String mapItemsJson;
  String mapTarget;
  num mapConcurrency;
  List<String> raceBranches;
  String raceWinner;
  String outputEventType;
  String outputDataJson;
  String inputPrompt;
  String configNameJson;
  String configMetadataJson;
  String subflowId;
  String subflowParametersJson;
  List<AssertAssertionDraft> assertAssertions;
  String transformBindingsJson;
  String auditActionJson;
  String auditActorJson;
  String auditTargetJson;
  String auditReasonJson;
  String checkpointName;
  String mutexName;
  num mutexPollInterval;
  String throttleName;
  num throttleMaxPerWindow;
  num throttleWindowSeconds;
  num throttlePollInterval;
  String awaitRunIdsJson;
  String awaitMode;
  num awaitPollInterval;
  String debounceName;
  num debounceDelaySeconds;
  String debounceTriggerKeyJson;
  String collectName;
  num collectMax;
  String barrierName;
  num barrierCount;
  num barrierPollInterval;
  String circuitName;
  num circuitThreshold;
  num circuitWindowSeconds;
  num circuitCooldownSeconds;
  String eventSourceType;
  String eventSourceFilterJson;
  num eventSourceMax;
  bool locked;
  bool skipped;
  num maxAttempts;
  num timeoutSeconds;
  String actionName;
  String actionFunction;
  String parametersJson;
  String transitionsJson;
}

StepEditorState createStepEditorState() => StepEditorState(
      switchValueJson: pretty(valueRef('params', ['mode'])),
      toggleValueJson: pretty(valueRef('config', ['flags', 'enabled'])),
      percentageKeyJson: pretty(valueRef('input', ['user_id'])),
      auditActionJson: pretty('workflow.audit'),
      awaitRunIdsJson: pretty(valueRef('params', ['run_ids'])),
    );

class TriggerJsonDraft {
  TriggerJsonDraft({required this.configuration, required this.metadata});

  String configuration;
  String metadata;
}

enum WorkflowEditorMode { graph, json, wdl }

/// large mutable state shared by catalog/editor/runs; mirrors the ts source's
/// WorkflowServicesState exactly, including its mutation style.
class WorkflowServicesState {
  WorkflowServicesState({
    required this.workflows,
    this.selectedWorkflowId,
    required this.workflowDraft,
    required this.workflowJson,
    required this.workflowWdl,
    required this.workflowWdlError,
    required this.workflowConcurrency,
    required this.workflowSettingsOpen,
    required this.runInputOpen,
    required this.runInputDraft,
    required this.runInputDebug,
    required this.workflowTriggers,
    required this.triggerEditorOpen,
    required this.triggerEditorCreating,
    required this.triggerEditorError,
    required this.triggerDraft,
    required this.triggerJson,
    required this.workflowEditorMode,
    required this.workflowLayoutDirection,
    this.workflowInspectorMode = 'step',
    required this.stepEditorOpen,
    required this.stepEditorCreating,
    required this.stepEditorCreatedNodeId,
    required this.stepEditorError,
    required this.stepEditor,
    required this.workflowRuns,
    required this.workflowLayoutVersion,
    this.selectedWorkflowRunId,
    this.workflowRunDetail,
    required this.openRunIds,
    required this.workflowRunGates,
    this.workflowRunGateRunId,
    required this.workflowRunGateFingerprint,
    required this.workflowNodeDetailExtra,
    required this.selectedStepId,
    required this.inlineEditNodeId,
    required this.selectedGraphEdgeId,
    required this.selectedWorkflowRunNodeId,
    this.selectedWorkflowNodeRunId,
    required this.isDirty,
    required this.watchExpressionsByWorkflowId,
  });

  List<WorkflowDefinition> workflows;
  String? selectedWorkflowId;
  WorkflowDefinition workflowDraft;
  String workflowJson;
  String workflowWdl;
  String workflowWdlError;
  num workflowConcurrency;
  bool workflowSettingsOpen;
  bool runInputOpen;
  JsonRecord runInputDraft;
  bool runInputDebug;
  List<WorkflowTrigger> workflowTriggers;
  bool triggerEditorOpen;
  bool triggerEditorCreating;
  String triggerEditorError;
  WorkflowTrigger triggerDraft;
  TriggerJsonDraft triggerJson;
  WorkflowEditorMode workflowEditorMode;
  WorkflowLayoutDirection workflowLayoutDirection;

  /// the ts source types this as the single-value literal `"step"`; kept as a
  /// plain String since nothing else is ever assigned to it in the source.
  String workflowInspectorMode;
  bool stepEditorOpen;
  bool stepEditorCreating;
  String stepEditorCreatedNodeId;
  String stepEditorError;
  StepEditorState stepEditor;
  List<RunSummary> workflowRuns;
  int workflowLayoutVersion;
  String? selectedWorkflowRunId;
  WorkflowRunDetail? workflowRunDetail;
  List<String> openRunIds;
  List<GateRecord> workflowRunGates;
  String? workflowRunGateRunId;
  String workflowRunGateFingerprint;
  String workflowNodeDetailExtra;
  String selectedStepId;
  String inlineEditNodeId;
  String selectedGraphEdgeId;
  String selectedWorkflowRunNodeId;
  String? selectedWorkflowNodeRunId;
  bool isDirty;
  Map<String, List<String>> watchExpressionsByWorkflowId;

  /// shallow copy (new top-level reference, same nested-object references) —
  /// mirrors the ts source's `{...current}` spread used by host.notify().
  WorkflowServicesState clone() => WorkflowServicesState(
        workflows: workflows,
        selectedWorkflowId: selectedWorkflowId,
        workflowDraft: workflowDraft,
        workflowJson: workflowJson,
        workflowWdl: workflowWdl,
        workflowWdlError: workflowWdlError,
        workflowConcurrency: workflowConcurrency,
        workflowSettingsOpen: workflowSettingsOpen,
        runInputOpen: runInputOpen,
        runInputDraft: runInputDraft,
        runInputDebug: runInputDebug,
        workflowTriggers: workflowTriggers,
        triggerEditorOpen: triggerEditorOpen,
        triggerEditorCreating: triggerEditorCreating,
        triggerEditorError: triggerEditorError,
        triggerDraft: triggerDraft,
        triggerJson: triggerJson,
        workflowEditorMode: workflowEditorMode,
        workflowLayoutDirection: workflowLayoutDirection,
        workflowInspectorMode: workflowInspectorMode,
        stepEditorOpen: stepEditorOpen,
        stepEditorCreating: stepEditorCreating,
        stepEditorCreatedNodeId: stepEditorCreatedNodeId,
        stepEditorError: stepEditorError,
        stepEditor: stepEditor,
        workflowRuns: workflowRuns,
        workflowLayoutVersion: workflowLayoutVersion,
        selectedWorkflowRunId: selectedWorkflowRunId,
        workflowRunDetail: workflowRunDetail,
        openRunIds: openRunIds,
        workflowRunGates: workflowRunGates,
        workflowRunGateRunId: workflowRunGateRunId,
        workflowRunGateFingerprint: workflowRunGateFingerprint,
        workflowNodeDetailExtra: workflowNodeDetailExtra,
        selectedStepId: selectedStepId,
        inlineEditNodeId: inlineEditNodeId,
        selectedGraphEdgeId: selectedGraphEdgeId,
        selectedWorkflowRunNodeId: selectedWorkflowRunNodeId,
        selectedWorkflowNodeRunId: selectedWorkflowNodeRunId,
        isDirty: isDirty,
        watchExpressionsByWorkflowId: watchExpressionsByWorkflowId,
      );
}

class PendingBreakpointPatch {
  const PendingBreakpointPatch({required this.runId, required this.breakpoints, required this.mutationId});

  final String runId;
  final List<String> breakpoints;
  final int mutationId;
}

/// bookkeeping not part of the reactive state (timers, request-fencing counters,
/// side-table maps) — mirrors the ts source's WorkflowServicesInternal exactly.
class WorkflowServicesInternal {
  final Map<String, WorkflowRunDetail?> runDetailById = {};
  final Map<String, int> latestWorkflowRunPushVersion = {};
  final Map<String, int> latestWorkflowRunHttpRequest = {};
  int nextWorkflowRunDetailVersion = 0;
  int nextWorkflowRunHttpRequestId = 0;
  int nextWorkflowRunGateRequestId = 0;
  int nextBreakpointMutationId = 0;
  PendingBreakpointPatch? pendingBreakpointPatch;
  Timer? workflowWdlSyncTimer;
  Timer? workflowJsonWriteReleaseTimer;
  Timer? workflowWdlWriteReleaseTimer;
  Timer? stepEditorApplyTimer;
  bool workflowJsonWriteGuard = false;
  bool workflowWdlWriteGuard = false;
  bool stepEditorHydrating = false;
  JsonRecord? stepEditorBaselineDefinition;
}

WorkflowServicesState createWorkflowServicesState() => WorkflowServicesState(
      workflows: [],
      selectedWorkflowId: null,
      workflowDraft: newWorkflowDraft(),
      workflowJson: '{}',
      workflowWdl: '',
      workflowWdlError: '',
      workflowConcurrency: 1,
      workflowSettingsOpen: false,
      runInputOpen: false,
      runInputDraft: {},
      runInputDebug: false,
      workflowTriggers: [],
      triggerEditorOpen: false,
      triggerEditorCreating: false,
      triggerEditorError: '',
      triggerDraft: newWorkflowTriggerDraft('', WorkflowTriggerKind.cron),
      triggerJson: TriggerJsonDraft(configuration: '{}', metadata: '{}'),
      workflowEditorMode: WorkflowEditorMode.graph,
      workflowLayoutDirection: WorkflowLayoutDirection.horizontal,
      stepEditorOpen: false,
      stepEditorCreating: false,
      stepEditorCreatedNodeId: '',
      stepEditorError: '',
      stepEditor: createStepEditorState(),
      workflowRuns: [],
      workflowLayoutVersion: 0,
      selectedWorkflowRunId: null,
      workflowRunDetail: null,
      openRunIds: [],
      workflowRunGates: [],
      workflowRunGateRunId: null,
      workflowRunGateFingerprint: '',
      workflowNodeDetailExtra: '',
      selectedStepId: '',
      inlineEditNodeId: '',
      selectedGraphEdgeId: '',
      selectedWorkflowRunNodeId: '',
      selectedWorkflowNodeRunId: null,
      isDirty: false,
      watchExpressionsByWorkflowId: {},
    );
