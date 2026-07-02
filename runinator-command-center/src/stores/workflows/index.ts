import { defineStore } from "pinia";
import { computed, reactive, ref, shallowReactive, shallowRef, watch } from "vue";
import {
  cancelWorkflowRun,
  closeGate,
  compileWdl,
  continueWorkflowRun,
  createWorkflowRun,
  decompileToWdl,
  deleteWorkflow,
  deleteWorkflowTrigger,
  duplicateWorkflow,
  downloadBlob,
  downloadTextFile,
  fetchGates,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  fetchWorkflowTriggers,
  fetchWorkflows,
  openGate,
  patchWorkflowRunDebug,
  pauseWorkflowRun,
  renameWorkflowRun as renameWorkflowRunApi,
  replayWorkflowRun as replayWorkflowRunApi,
  resumeWorkflowRun,
  rerunWorkflowNode,
  runToCursorWorkflowRun,
  saveWorkflowWdl,
  saveWorkflowTrigger,
  skipWorkflowNode,
  stepWorkflowRun,
  type WorkflowDebugPatch,
  type WorkflowWdlSaveRequest,
} from "../../api/commandCenterApi";
import type {
  Connection,
  Edge,
  EdgeChange,
  EdgeMouseEvent,
  EdgeUpdateEvent,
  Node,
  NodeChange,
  NodeDragEvent,
  NodeMouseEvent,
} from "@vue-flow/core";
import type {
  ControlFrame,
  DebugFrame,
  GateRecord,
  JsonRecord,
  JsonValue,
  ProviderMetadata,
  RunArtifact,
  RunChunk,
  RunSummary,
  RuninatorType,
  WorkflowDefinition,
  WorkflowEdgeEditorDraft,
  WorkflowEditorEdgeData,
  WorkflowLayoutDirection,
  WorkflowNodeKind,
  WorkflowNodeRun,
  WorkflowRunDetail,
  WorkflowTrigger,
  WorkflowTriggerKind,
  WorkflowValidationIssue,
} from "../../types/models";
import { asJsonValue } from "../../types/json";
import { runWorkflowSnapshot, workflowInputType } from "../../types/models";
import { coerceControlFrame, coerceDebugFrame } from "../../types/models/workflow-state";
import { pretty } from "../../utils/format";
import { cloneJson, parseObject, parseRequiredJson, parseRequiredObject } from "../../utils/json";
import { displayValue, isBlankValue } from "../../utils/values";
import { createZip, type ZipEntry } from "../../utils/zip";
import {
  applyWorkflowEdgeEditorDraft,
  applyWorkflowInlineNodeEdit,
  asArray,
  asRecord,
  isRecord,
  autoArrangeWorkflowEdgeHandles,
  autoArrangeWorkflowLayout,
  buildGraphEdges,
  buildGraphNodes,
  createWorkflowNode,
  directTransitionKeys,
  isSameConnectionPointLoop,
  nodeRef,
  nodeRefId,
  normalizeWorkflowDefinition,
  parameterSemanticKey,
  removeConditionBranch,
  removeWorkflowEdge,
  removeWorkflowEdgeHandles,
  removeWorkflowNodeReferences,
  setConditionBranch,
  setWorkflowEdgeHandles,
  setWorkflowEdgeLabelAnchor,
  setWorkflowEdgeLabelOffset,
  moveWorkflowEdgeEditorDraft,
  optionIdForSourceHandle,
  workflowEdgeOptionId,
  workflowEdgeEditorDraft,
  workflowEdgeSemanticOptions,
  uniqueWorkflowNodeId,
  validateWorkflowIssues,
  validateWorkflowReferenceSyntax,
  valueRef,
  workflowNodeActionConfig,
  workflowNodeActionInputs,
  workflowNodeKinds,
  workflowRunSearchText,
} from "../../utils/workflows";
import {
  branchPolicyName,
  buildInputSkeleton,
  boundedIndex,
  defaultEdgeEditorDraft,
  defaultTriggerConfiguration,
  errorMessage,
  formatMaybeDate,
  dateTimeLocalToIso,
  isLockedWorkflowNode,
  isProtectedWorkflowNode,
  newWorkflowDraft,
  newWorkflowTriggerDraft,
  nextNodePosition,
  nodeRefArray,
  switchCaseEditor,
  validateJsonValueType,
  type SwitchCaseEditor,
} from "./helpers";
import { useAppStore } from "../app";
import { useProvidersStore } from "../providers";
export { buildInputSkeleton, newWorkflowDraft, newWorkflowTriggerDraft } from "./helpers";
import { useResourcesStore } from "../resources";

const WORKFLOW_WDL_SYNC_DELAY_MS = 1500;

function providerCatalog(): ProviderMetadata[] {
  return useProvidersStore().providers;
}

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = shallowRef<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<string | null>(null);
  const workflowDraft = reactive(newWorkflowDraft());
  const workflowJson = ref("{}");
  const workflowWdl = ref("");
  // populated when the current draft cannot be decompiled to wdl; the wdl pane goes read-only and
  // shows this message so an incomplete graph never compiles empty wdl back over the definition.
  const workflowWdlError = ref("");
  const workflowConcurrency = ref(1);
  const workflowSettingsOpen = ref(false);
  const runInputOpen = ref(false);
  const runInputDraft = ref<JsonRecord>({});
  const runInputDebug = ref(false);
  const workflowTriggers = ref<WorkflowTrigger[]>([]);
  const triggerEditorOpen = ref(false);
  const triggerEditorCreating = ref(false);
  const triggerEditorError = ref("");
  const triggerDraft = reactive<WorkflowTrigger>(newWorkflowTriggerDraft("", "cron"));
  const triggerJson = reactive({ configuration: "{}", metadata: "{}" });
  const workflowEditorMode = ref<"graph" | "json" | "wdl">("graph");
  const workflowLayoutDirection = ref<WorkflowLayoutDirection>("horizontal");
  const workflowInspectorMode = ref<"step">("step");
  const stepEditorOpen = ref(false);
  const stepEditorCreating = ref(false);
  const stepEditorCreatedNodeId = ref("");
  const stepEditorError = ref("");
  const workflowRuns = shallowRef<RunSummary[]>([]);
  const workflowLayoutVersion = ref(0);
  const recentWorkflowRuns = computed((): RunSummary[] => {
    const query = app.normalizedSearch;
    const runs = workflowRuns.value;

    if (!query) {
      return runs.slice(0, 50);
    }

    const matches: RunSummary[] = [];

    for (const run of runs) {
      const workflowName = workflowNameForRun(run);

      if (workflowRunSearchText(run, workflowName).includes(query)) {
        matches.push(run);
      }
    }

    return matches.slice(0, 50);
  });
  const selectedWorkflowRunId = ref<string | null>(null);
  const workflowRunDetail = shallowRef<WorkflowRunDetail | null>(null);
  const openRunIds = ref<string[]>([]);
  type RunDetailMap = Map<string, WorkflowRunDetail | null>;
  const runDetailById = shallowReactive(new Map<string, WorkflowRunDetail | null>()) as RunDetailMap;
  const MAX_OPEN_RUN_TABS = 8;
  const latestWorkflowRunPushVersion = new Map<string, number>();
  const latestWorkflowRunHttpRequest = new Map<string, number>();
  let nextWorkflowRunDetailVersion = 0;
  let nextWorkflowRunHttpRequestId = 0;
  const workflowRunGates = shallowRef<GateRecord[]>([]);
  const workflowRunGateRunId = ref<string | null>(null);
  const workflowRunGateFingerprint = ref("");
  let nextWorkflowRunGateRequestId = 0;
  let nextBreakpointMutationId = 0;
  let pendingBreakpointPatch: { runId: string; breakpoints: string[]; mutationId: number } | null =
    null;
  const workflowNodeDetailExtra = ref("");
  const selectedStepId = ref("");
  // the node currently showing its inline mini-editor; distinct from selection so a single
  // click only selects while a double-click opens the inline form.
  const inlineEditNodeId = ref("");
  const selectedGraphEdgeId = ref("");
  const selectedWorkflowRunNodeId = ref("");
  const selectedWorkflowNodeRunId = ref<string | null>(null);
  const stepEditor = reactive({
    id: "",
    name: "",
    kind: "action",
    approval_type: "generic",
    approval_prompt: "Approval required",
    gate_kind: "manual",
    gate_when_json: "{}",
    gate_poll_interval: 30,
    gate_timeout: 0,
    gate_label: "",
    signal_name: "signal",
    condition_fallback: "",
    condition_branches: [] as { when_json: string; target: string }[],
    wait_seconds: 60,
    wait_initial_status: "waiting",
    wait_until_status: "",
    wait_json: "{}",
    loop_items_json: "[]",
    loop_target: "",
    loop_max_iterations: 10,
    switch_value_json: pretty(valueRef("params", ["mode"])),
    switch_cases: [] as SwitchCaseEditor[],
    switch_default: "",
    toggle_value_json: pretty(valueRef("config", ["flags", "enabled"])),
    toggle_on: "",
    toggle_off: "",
    percentage_key_json: pretty(valueRef("input", ["user_id"])),
    percentage_buckets: [] as { weight: number; target: string }[],
    percentage_default: "",
    parallel_branches: [] as string[],
    join_wait_for: [] as string[],
    join_mode: "all",
    try_body: "",
    try_catch: "",
    try_finally: "",
    map_items_json: "[]",
    map_target: "",
    map_concurrency: 1,
    race_branches: [] as string[],
    race_winner: "first_success",
    output_event_type: "workflow.output",
    output_data_json: "{}",
    input_prompt: "Provide input",
    config_name_json: '""',
    config_metadata_json: "{}",
    subflow_id: "",
    subflow_parameters_json: "{}",
    assert_assertions: [] as { name: string; condition_json: string; message: string }[],
    transform_bindings_json: "{}",
    audit_action_json: pretty("workflow.audit"),
    audit_actor_json: "",
    audit_target_json: "",
    audit_reason_json: "",
    checkpoint_name: "",
    mutex_name: "",
    mutex_poll_interval: 30,
    throttle_name: "",
    throttle_max_per_window: 10,
    throttle_window_seconds: 60,
    throttle_poll_interval: 30,
    await_run_ids_json: pretty(valueRef("params", ["run_ids"])),
    await_mode: "all",
    await_poll_interval: 30,
    debounce_name: "",
    debounce_delay_seconds: 30,
    debounce_trigger_key_json: "",
    collect_name: "",
    collect_max: 10,
    barrier_name: "",
    barrier_count: 2,
    barrier_poll_interval: 30,
    circuit_name: "",
    circuit_threshold: 5,
    circuit_window_seconds: 60,
    circuit_cooldown_seconds: 60,
    event_source_type: "*",
    event_source_filter_json: "",
    event_source_max: 0,
    locked: false,
    skipped: false,
    max_attempts: 1,
    timeout_seconds: 0,
    action_name: "",
    action_function: "",
    parameters_json: "{}",
    transitions_json: "{}",
  });

  const isDirty = ref(false);
  let workflowWdlSyncTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowJsonWriteReleaseTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowWdlWriteReleaseTimer: ReturnType<typeof setTimeout> | null = null;
  let stepEditorApplyTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowJsonWriteGuard = false;
  let workflowWdlWriteGuard = false;
  let stepEditorHydrating = false;
  let stepEditorBaselineDefinition: JsonRecord | null = null;

  const app = useAppStore();
  watch(workflowJson, () => {
    if (workflowJsonWriteGuard || workflowEditorMode.value !== "json") {
      return;
    }

    scheduleWorkflowJsonSync();
  });
  watch(workflowWdl, () => {
    // the graph and wdl panes are live side by side; a user wdl edit always compiles into the draft.
    // a silent programmatic write (guard) or a non-representable draft (error) must not.
    if (workflowWdlWriteGuard || workflowWdlError.value) {
      return;
    }

    workflowEditorMode.value = "wdl";
    scheduleWorkflowWdlSync();
  });
  watch(
    stepEditor,
    () => {
      if (stepEditorHydrating || !stepEditorOpen.value) {
        return;
      }

      scheduleStepEditorApply();
    },
    { deep: true },
  );
  const selectedWorkflow = computed((): WorkflowDefinition | null =>
    workflows.value.find((workflow) => workflow.id === selectedWorkflowId.value) ?? null,
  );
  const canRunWorkflow = computed(() =>
    Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id),
  );
  const selectedWorkflowInputType = computed<RuninatorType | null>(() =>
    selectedWorkflow.value ? workflowInputType(selectedWorkflow.value) : null,
  );
  const selectedWorkflowHasInputs = computed(() => {
    const ty = selectedWorkflowInputType.value;
    return ty?.type === "struct" && Object.keys(ty.fields).length > 0;
  });
  const canManageWorkflowTriggers = computed(() => Boolean(workflowDraft.id));
  const canStepWorkflowRun = computed(() => workflowRunDetail.value?.run.status === "debug_paused");
  const debugState = computed<DebugFrame | null>(() => {
    return coerceDebugFrame(workflowRunDetail.value?.run.state?.debug) ?? null;
  });
  const isDebugRun = computed(() => Boolean(debugState.value?.enabled));
  const canContinueWorkflowRun = computed(
    () => workflowRunDetail.value?.run.status === "debug_paused",
  );
  const controlState = computed<ControlFrame | null>(() => {
    return coerceControlFrame(workflowRunDetail.value?.run.state?.control) ?? null;
  });
  const pauseRequested = computed(() => Boolean(controlState.value?.pause_requested));
  const canPauseWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;
    return Boolean(
      status &&
      ["running", "waiting", "approval_required"].includes(status) &&
      !pauseRequested.value,
    );
  });
  const canResumeWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;
    return status === "paused" || (status === "debug_paused" && pauseRequested.value);
  });
  const canCancelWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;

    if (!status) {
      return false;
    }

    return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
  });
  const currentBreakpoints = computed<string[]>(() => debugState.value?.breakpoints ?? []);

  function isBreakpointed(nodeId: string): boolean {
    return currentBreakpoints.value.includes(nodeId);
  }

  const selectedStepKindLocked = computed(() => {
    const node = asArray(workflowDraft.definition.nodes)
      .filter(isRecord)
      .find((item) => item.id === selectedStepId.value);
    return isLockedWorkflowNode(node);
  });
  const canRemoveSelectedStep = computed(() => {
    const node = asArray(workflowDraft.definition.nodes)
      .filter(isRecord)
      .find((item) => item.id === selectedStepId.value);
    return Boolean(node && !isLockedWorkflowNode(node));
  });
  const filteredWorkflows = computed((): WorkflowDefinition[] => {
    const query = app.normalizedSearch;

    if (!query) {
      return workflows.value;
    }

    return workflows.value.filter((workflow) =>
      [workflow.name, workflow.id ?? "", workflow.version].some((value) =>
        value.toLowerCase().includes(query),
      ),
    );
  });
  const workflowRunDetailText = computed(() => {
    const detail = workflowRunDetail.value;

    if (!detail) {
      return "";
    }

    const lines = [
      `Run ${detail.run.id}: ${detail.run.status}`,
      `Started: ${formatMaybeDate(detail.run.started_at)}`,
      `Finished: ${formatMaybeDate(detail.run.finished_at)}`,
    ];

    if (detail.run.message) {
      lines.push(`Message: ${detail.run.message}`);
    }

    for (const step of detail.nodes) {
      lines.push(
        `${step.node_id}: ${step.status}, attempt ${String(step.attempt)}, node run ${step.id}${step.message ? `, ${step.message}` : ""}`,
      );
    }

    return `${lines.join("\n")}${workflowNodeDetailExtra.value}`;
  });
  const stepNeeds = computed(() => {
    const transitions = parseObject(stepEditor.transitions_json, {});
    return ["next", "on_success", "on_failure", "on_timeout", "on_reject"]
      .filter((key) => transitions[key])
      .map((key) => `${key}:${nodeRefId(transitions[key]) ?? "invalid"}`)
      .join(",");
  });
  const subflowNames = computed(
    (): Map<string, string> =>
      new Map(workflows.value.flatMap((w) => (w.id != null ? [[w.id, w.name] as const] : []))),
  );
  const graphNodes = computed((): Node[] =>
    buildGraphNodes(workflowDraft, null, subflowNames.value, providerCatalog()),
  );
  const graphEdges = computed((): Edge[] => buildGraphEdges(workflowDraft));
  const graphValidationIssues = computed((): WorkflowValidationIssue[] =>
    validateWorkflowIssues(workflowDraft.definition, providerCatalog()),
  );
  const workflowRunWorkflow = computed((): WorkflowDefinition | null => {
    const snapshot = runWorkflowSnapshot(workflowRunDetail.value);

    if (snapshot) {
      return snapshot;
    }

    const workflowId =
      workflowRunDetail.value?.run.workflow_id ??
      workflowRuns.value.find((run) => run.id === selectedWorkflowRunId.value)?.workflow_id;
    const items = workflows.value;

    for (const workflow of items) {
      if (workflow.id === workflowId) {
        return workflow;
      }
    }

    return null;
  });
  const workflowRunGatesByNodeId = computed((): Map<string, GateRecord> => {
    const gates = new Map<string, GateRecord>();

    for (const gate of workflowRunGates.value) {
      if (typeof gate.node_id === "string" && gate.node_id.length > 0) {
        gates.set(gate.node_id, gate);
      }
    }

    return gates;
  });
  const runGraphNodes = computed((): Node[] => {
    if (!workflowRunWorkflow.value) {
      return [];
    }

    return buildGraphNodes(
      workflowRunWorkflow.value,
      workflowRunDetail.value,
      subflowNames.value,
      providerCatalog(),
    ).map((node) => ({
      ...node,
      data: {
        ...(node.data as JsonRecord),
        readOnly: true,
        allowGateResolution: true,
        gate: workflowRunGatesByNodeId.value.get(node.id) ?? null,
      },
    }));
  });
  const runGraphEdges = computed((): Edge[] =>
    workflowRunWorkflow.value ? buildGraphEdges(workflowRunWorkflow.value) : [],
  );
  const selectedNode = computed((): JsonRecord | null =>
    ensureWorkflowNodes().find((item) => item.id === selectedStepId.value) ?? null,
  );
  const selectedGraphEdge = computed(
    () => graphEdges.value.find((edge: Edge) => edge.id === selectedGraphEdgeId.value) ?? null,
  );
  const selectedNodeIssues = computed<WorkflowValidationIssue[]>(() =>
    graphValidationIssues.value.filter((issue) => issue.nodeId === selectedStepId.value),
  );
  const selectedEdgeIssues = computed<WorkflowValidationIssue[]>(() => {
    const edge = selectedGraphEdge.value;

    if (!edge) {
      return [];
    }

    const data = edge.data as WorkflowEditorEdgeData | undefined;
    const semanticKey =
      data?.transitionKey ??
      (typeof data?.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    return graphValidationIssues.value.filter(
      (issue) => issue.edgeKey === `${edge.source}:${semanticKey}`,
    );
  });
  const selectedNodePendingApproval = computed((): WorkflowNodeRun | null => {
    const detail = workflowRunDetail.value;

    if (!detail || !selectedStepId.value) {
      return null;
    }

    return (
      detail.nodes
        .filter(
          (node) =>
            node.node_id === selectedStepId.value &&
            ["waiting", "approval_required", "pending"].includes(node.status),
        )
        .at(-1) ?? null
    );
  });

  async function refreshWorkflows() {
    console.info("[command-center] refreshing workflows");
    workflows.value = (await app
      .runOperation("Refreshing workflows", () => fetchWorkflows())
      .catch(() => [])) as WorkflowDefinition[];

    if (!selectedWorkflowId.value && workflows.value.length > 0) {
      selectedWorkflowId.value = workflows.value[0].id;
    }

    const items = workflows.value;
    let workflow: WorkflowDefinition | undefined;

    for (const item of items) {
      if (item.id === selectedWorkflowId.value) {
        workflow = item;
        break;
      }
    }

    workflow ??= items[0];

    if (!isDirty.value) {
      await selectWorkflow(workflow);
    }
  }

  function clearServiceState(options: { discardDraft?: boolean } = {}) {
    workflows.value = [];
    workflowRuns.value = [];
    workflowRunDetail.value = null;
    openRunIds.value = [];
    runDetailById.clear();
    pendingBreakpointPatch = null;
    workflowNodeDetailExtra.value = "";
    selectedWorkflowRunId.value = null;
    selectedWorkflowRunNodeId.value = "";
    selectedWorkflowNodeRunId.value = null;
    clearWorkflowRunGates();
    clearWorkflowTriggerState();

    if (isDirty.value && !options.discardDraft) {
      return;
    }

    isDirty.value = false;
    selectedWorkflowId.value = null;
    Object.assign(workflowDraft, newWorkflowDraft());
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    setWorkflowWdlSilently("");
    workflowWdlError.value = "";
    selectedStepId.value = "";
    stepEditorOpen.value = false;
  }

  function getTransition(key: string): string {
    const transitions = parseObject(stepEditor.transitions_json, {});
    return nodeRefId(transitions[key]) ?? "";
  }

  function setTransition(key: string, value: string) {
    const transitions = parseObject(stepEditor.transitions_json, {});

    if (value) {
      transitions[key] = nodeRef(value);
    } else {
      stepEditor.transitions_json = pretty(
        Object.fromEntries(Object.entries(transitions).filter(([entryKey]) => entryKey !== key)),
      );
      isDirty.value = true;
      return;
    }

    stepEditor.transitions_json = pretty(transitions);
    isDirty.value = true;
  }

  function selectWorkflow(workflow: WorkflowDefinition) {
    const isSwitch = selectedWorkflowId.value !== workflow.id;
    selectedWorkflowId.value = workflow.id;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflow)));
    workflowConcurrency.value = Number(workflowDraft.definition.concurrency ?? 1);
    setWorkflowJsonSilently(pretty(workflowDraft.definition));

    if (isSwitch) {
      selectedStepId.value = "";
      clearWorkflowTriggerState();
      stepEditorOpen.value = false;
    }

    workflowEditorMode.value = "graph";
    isDirty.value = false;
    // the graph derives from the draft; the wdl pane is decompiled, so refresh it for the newly
    // selected workflow since both panes are visible at once.
    return refreshWorkflowWdl();
  }

  function addWorkflow() {
    const workflow = newWorkflowDraft();
    workflows.value.push(workflow);
    void selectWorkflow(workflow);
  }

  // open the input form when the workflow declares inputs, otherwise launch immediately.
  async function runSelectedWorkflow(debug = false) {
    const workflow = selectedWorkflow.value;

    if (!workflow?.id || !workflow.enabled) {
      app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
      return;
    }

    if (selectedWorkflowHasInputs.value) {
      runInputDraft.value = buildInputSkeleton(selectedWorkflowInputType.value);
      runInputDebug.value = debug;
      runInputOpen.value = true;
      return;
    }

    await launchWorkflowRun(debug, {});
  }

  async function runSelectedWorkflowDebug() {
    return runSelectedWorkflow(true);
  }

  function closeRunInput() {
    runInputOpen.value = false;
  }

  async function confirmRunInput() {
    const debug = runInputDebug.value;
    const parameters = runInputDraft.value;
    runInputOpen.value = false;
    await launchWorkflowRun(debug, parameters);
  }

  async function launchWorkflowRun(debug: boolean, parameters: JsonRecord) {
    const workflow = selectedWorkflow.value;

    const workflowId = workflow?.id;

    if (!workflowId || !workflow.enabled) {
      app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
      return;
    }

    const response = await app.runOperation(
      debug
        ? `Running workflow ${workflow.name} in debug mode`
        : `Running workflow ${workflow.name}`,
      () => createWorkflowRun(workflowId, { debug, parameters }),
    );
    selectedWorkflowRunId.value = response.id;
    app.setStatus(`${debug ? "Debug workflow run" : "Workflow run"} queued: ${response.id}`);
    await fetchWorkflowRunDetail(response.id);
    await fetchRecentWorkflowRuns();
    app.activeTab = "Runs";
  }

  async function stepSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Stepping workflow run ${runId}`, () =>
      stepWorkflowRun(runId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to step workflow run");
      return;
    }

    app.setStatus(response.message || `Workflow run ${runId} stepped`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function continueSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canContinueWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Continuing workflow run ${runId}`, () =>
      continueWorkflowRun(runId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to continue workflow run");
      return;
    }

    app.setStatus(response.message || `Workflow run ${runId} continued`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function cancelSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canCancelWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Canceling workflow run ${runId}`, () =>
      cancelWorkflowRun(runId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to cancel workflow run");
      return;
    }

    app.setStatus(response.message || `Workflow run ${runId} canceled`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function pauseSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canPauseWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Pausing workflow run ${runId}`, () =>
      pauseWorkflowRun(runId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to pause workflow run");
      return;
    }

    app.setStatus(response.message || `Workflow run ${runId} pause requested`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function resumeSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canResumeWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Resuming workflow run ${runId}`, () =>
      resumeWorkflowRun(runId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to resume workflow run");
      return;
    }

    app.setStatus(response.message || `Workflow run ${runId} resumed`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function patchSelectedWorkflowRunDebug(patch: WorkflowDebugPatch) {
    if (!workflowRunDetail.value || !isDebugRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Updating debug settings for run ${runId}`, () =>
      patchWorkflowRunDebug(runId, patch),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to update debug settings");
      return;
    }

    await fetchWorkflowRunDetail(runId, true);
  }

  async function toggleBreakpoint(nodeId: string) {
    if (!workflowRunDetail.value || !isDebugRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const current = currentBreakpoints.value;
    const next = current.includes(nodeId)
      ? current.filter((id) => id !== nodeId)
      : [...current, nodeId];
    const mutationId = ++nextBreakpointMutationId;
    pendingBreakpointPatch = { runId, breakpoints: next, mutationId };
    applyBreakpointPatch(workflowRunDetail.value, next);

    try {
      const response = await app.runOperation(`Updating debug settings for run ${runId}`, () =>
        patchWorkflowRunDebug(runId, { breakpoints: next }),
      );

      if (!response.success) {
        app.setError(response.message || "Failed to update debug settings");

        if (clearPendingBreakpointPatch(runId, mutationId)) {
          applyBreakpointPatch(workflowRunDetail.value, current);
        }

        return;
      }

      await fetchWorkflowRunDetail(runId, true);
    } catch {
      if (clearPendingBreakpointPatch(runId, mutationId)) {
        applyBreakpointPatch(workflowRunDetail.value, current);
      }
    }
  }

  async function runToCursor(nodeId: string) {
    if (!workflowRunDetail.value || !isDebugRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Running to cursor ${nodeId}`, () =>
      runToCursorWorkflowRun(runId, nodeId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to run to cursor");
      return;
    }

    app.setStatus(response.message || `Running to ${nodeId}`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function skipCurrentNode(outputJson: unknown, message?: string) {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Skipping current node`, () =>
      skipWorkflowNode(runId, outputJson, message),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to skip node");
      return;
    }

    app.setStatus(response.message || `Node skipped`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function rerunCurrentNode(parameters: unknown) {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) {
      return;
    }

    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Re-running current node`, () =>
      rerunWorkflowNode(runId, parameters),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to re-run node");
      return;
    }

    app.setStatus(response.message || `Node re-running`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function replaySelectedWorkflowRun(runId?: string, fromStepId?: string) {
    const targetId = runId ?? workflowRunDetail.value?.run.id;

    if (!targetId) {
      return;
    }

    const label = fromStepId
      ? `Replaying workflow run ${targetId} from step ${fromStepId}`
      : `Replaying workflow run ${targetId}`;
    const created = await app
      .runOperation(label, () => replayWorkflowRunApi(targetId, { fromStepId }))
      .catch((error: unknown) => {
        app.setError(String(error));
        return null;
      });

    if (!created?.id) {
      app.setError("Failed to start replay");
      return;
    }

    app.setStatus(`Replay started as run ${created.id}`);
    openRunInTab(created.id);
    activateRunTab(created.id);
    await fetchWorkflowRunDetail(created.id);
    await fetchRecentWorkflowRuns();
    app.activeTab = "Runs";
    return created.id;
  }

  async function renameSelectedWorkflowRun(runId: string, name: string | null) {
    if (!runId) {
      return;
    }

    const response = await app
      .runOperation(`Renaming run ${runId}`, () => renameWorkflowRunApi(runId, name))
      .catch((error: unknown) => {
        app.setError(String(error));
        return null;
      });

    if (!response) {
      return;
    }

    app.setStatus(response.message || `Run renamed`);
    await fetchRecentWorkflowRuns();

    if (workflowRunDetail.value?.run.id === runId) {
      await fetchWorkflowRunDetail(runId, true);
    }
  }

  // watch expressions persisted per workflow id in localStorage.
  const WATCH_STORAGE_PREFIX = "runinator.watch.";
  const watchExpressionsByWorkflowId = ref<Record<string, string[]>>(loadAllWatchExpressions());

  function loadAllWatchExpressions(): Record<string, string[]> {
    const storage = typeof window !== "undefined" ? window.localStorage : undefined;

    if (!storage) {
      return {};
    }

    const result: Record<string, string[]> = {};

    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);

      if (!key?.startsWith(WATCH_STORAGE_PREFIX)) {
        continue;
      }

      const id = key.slice(WATCH_STORAGE_PREFIX.length);

      if (!id) {
        continue;
      }

      try {
        const parsed: unknown = JSON.parse(storage.getItem(key) ?? "[]");

        if (Array.isArray(parsed)) {
          result[id] = parsed.filter((v): v is string => typeof v === "string");
        }
      } catch {
        // ignore
      }
    }

    return result;
  }

  const watchExpressionsForActiveWorkflow = computed<string[]>(() => {
    const workflowId = workflowRunWorkflow.value?.id;

    if (!workflowId) {
      return [];
    }

    return watchExpressionsByWorkflowId.value[workflowId] ?? [];
  });

  function persistWatchExpressions(workflowId: string, list: string[]) {
    const storage = typeof window !== "undefined" ? window.localStorage : undefined;

    if (!storage) {
      return;
    }

    storage.setItem(`${WATCH_STORAGE_PREFIX}${workflowId}`, JSON.stringify(list));
  }

  function addWatchExpression(expression: string) {
    const workflowId = workflowRunWorkflow.value?.id;

    if (!workflowId || !expression.trim()) {
      return;
    }

    const existing = watchExpressionsByWorkflowId.value[workflowId] ?? [];

    if (existing.includes(expression)) {
      return;
    }

    const next = [...existing, expression];
    watchExpressionsByWorkflowId.value = {
      ...watchExpressionsByWorkflowId.value,
      [workflowId]: next,
    };
    persistWatchExpressions(workflowId, next);
  }

  function removeWatchExpression(expression: string) {
    const workflowId = workflowRunWorkflow.value?.id;

    if (!workflowId) {
      return;
    }

    const existing = watchExpressionsByWorkflowId.value[workflowId] ?? [];
    const next = existing.filter((e) => e !== expression);
    watchExpressionsByWorkflowId.value = {
      ...watchExpressionsByWorkflowId.value,
      [workflowId]: next,
    };
    persistWatchExpressions(workflowId, next);
  }

  async function fetchWorkflowRunsForSelected(workflowId: string) {
    console.info("[command-center] refreshing workflow runs", { workflowId });
    workflowRuns.value = (await app
      .runOperation("Loading workflow runs", () => fetchWorkflowRuns(workflowId))
      .catch(() => [])) as RunSummary[];

    if (!workflowRuns.value.some((run) => run.id === selectedWorkflowRunId.value)) {
      selectedWorkflowRunId.value = workflowRuns.value[0]?.id ?? null;
    }
  }

  async function fetchRecentWorkflowRuns() {
    console.info("[command-center] refreshing recent workflow runs");
    workflowRuns.value = (await app
      .runOperation("Loading workflow runs", () => fetchWorkflowRuns())
      .catch(() => [])) as RunSummary[];
    const previousRunId = selectedWorkflowRunId.value;

    if (selectedWorkflowRunId.value === null && workflowRuns.value.length > 0) {
      const first = workflowRuns.value[0]?.id ?? null;

      if (first) {
        openRunInTab(first);
        activateRunTab(first);
      }
    }

    const currentRunId = selectedWorkflowRunId.value;

    if (currentRunId !== null && (!workflowRunDetail.value || previousRunId !== currentRunId)) {
      await fetchWorkflowRunDetail(currentRunId, true);
    }
  }

  async function selectWorkflowRun(run: RunSummary) {
    openRunInTab(run.id);
    activateRunTab(run.id);
    return fetchWorkflowRunDetail(run.id);
  }

  function openRunInTab(runId: string) {
    if (!runId) {
      return;
    }

    const ids = openRunIds.value;

    if (!ids.includes(runId)) {
      // Cap the tab count by evicting the oldest non-active tab.
      if (ids.length >= MAX_OPEN_RUN_TABS) {
        const victim = ids.find((id) => id !== selectedWorkflowRunId.value);

        if (victim) {
          closeRunTab(victim);
        }
      }

      openRunIds.value = [...ids, runId];
    }

    if (!runDetailById.has(runId)) {
      runDetailById.set(runId, null);
    }
  }

  function activateRunTab(runId: string) {
    if (!runId) {
      return;
    }

    if (!openRunIds.value.includes(runId)) {
      openRunInTab(runId);
    }

    selectedWorkflowRunId.value = runId;
    const tabDetail = runDetailById.get(runId) ?? null;
    workflowRunDetail.value = tabDetail;
    workflowNodeDetailExtra.value = "";
    selectedWorkflowRunNodeId.value = tabDetail?.nodes[0]?.node_id ?? "";

    if (tabDetail) {
      void syncWorkflowRunGatesForDetail(tabDetail);
    } else {
      clearWorkflowRunGates();
    }

    if (!runDetailById.get(runId)) {
      void fetchWorkflowRunDetail(runId, true);
    }
  }

  function closeRunTab(runId: string) {
    const ids = openRunIds.value;
    const index = ids.indexOf(runId);

    if (index === -1) {
      return;
    }

    const next = [...ids.slice(0, index), ...ids.slice(index + 1)];
    openRunIds.value = next;
    runDetailById.delete(runId);
    latestWorkflowRunPushVersion.delete(runId);
    latestWorkflowRunHttpRequest.delete(runId);

    if (selectedWorkflowRunId.value === runId) {
      const replacement = next[Math.min(index, next.length - 1)] ?? null;

      if (replacement) {
        activateRunTab(replacement);
      } else {
        selectedWorkflowRunId.value = null;
        workflowRunDetail.value = null;
        selectedWorkflowRunNodeId.value = "";
        clearWorkflowRunGates();
      }
    }
  }

  async function fetchWorkflowRunDetail(workflowRunId: string, silent = false) {
    console.info("[command-center] refreshing workflow run detail", { workflowRunId, silent });
    const requestStartedVersion = ++nextWorkflowRunDetailVersion;
    const requestId = ++nextWorkflowRunHttpRequestId;
    latestWorkflowRunHttpRequest.set(workflowRunId, requestId);
    const detail = silent
      ? await fetchWorkflowRun(workflowRunId).catch(() => null)
      : await app
          .runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId))
          .catch(() => null);
    applyWorkflowRunDetail(detail, { source: "http", requestStartedVersion, requestId });
  }

  function setWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    if (detail) {
      latestWorkflowRunPushVersion.set(detail.run.id, ++nextWorkflowRunDetailVersion);
    }

    applyWorkflowRunDetail(detail, { source: "ws" });
  }

  function selectWorkflowRunNode(nodeId: string) {
    selectedWorkflowRunNodeId.value = nodeId;
    void updateSelectedWorkflowNodeDetail();
  }

  function clearWorkflowRunGates() {
    workflowRunGates.value = [];
    workflowRunGateRunId.value = null;
    workflowRunGateFingerprint.value = "";
  }

  function workflowRunGateIds(detail: { nodes: { state?: JsonRecord }[] } | null): string[] {
    if (!detail) {
      return [];
    }

    const ids = detail.nodes
      .map((node) => node.state?.gate_id)
      .filter((value): value is string => typeof value === "string" && value.length > 0);
    return [...new Set(ids)].sort();
  }

  function workflowRunGateFingerprintForDetail(
    detail: { nodes: { state?: JsonRecord }[] } | null,
  ): string {
    return workflowRunGateIds(detail).join(",");
  }

  async function refreshWorkflowRunGates(runId: string, force = false) {
    const activeDetail =
      runId === workflowRunDetail.value?.run.id
        ? workflowRunDetail.value
        : (runDetailById.get(runId) ?? null);
    const fingerprint = workflowRunGateFingerprintForDetail(activeDetail);

    if (
      !force &&
      workflowRunGateRunId.value === runId &&
      workflowRunGateFingerprint.value === fingerprint
    ) {
      return;
    }

    const requestId = ++nextWorkflowRunGateRequestId;
    const gates = await fetchGates(runId).catch(() => null);

    if (requestId !== nextWorkflowRunGateRequestId) {
      return;
    }

    if (selectedWorkflowRunId.value !== runId && workflowRunDetail.value?.run.id !== runId) {
      return;
    }

    workflowRunGates.value = asArray(gates).filter(isRecord) as unknown as GateRecord[];
    workflowRunGateRunId.value = runId;
    workflowRunGateFingerprint.value = fingerprint;
  }

  async function syncWorkflowRunGatesForDetail(detail: WorkflowRunDetail | null, force = false) {
    if (!detail) {
      clearWorkflowRunGates();
      return;
    }

    await refreshWorkflowRunGates(detail.run.id, force);
  }

  async function resolveWorkflowRunGate(gateId: string, action: "open" | "close", reason?: string) {
    const runId = workflowRunDetail.value?.run.id ?? selectedWorkflowRunId.value;

    if (!runId) {
      app.setError("No workflow run selected");
      return;
    }

    const trimmed = reason?.trim() ? reason.trim() : undefined;
    const response = await app.runOperation(
      action === "open" ? "Opening gate" : "Closing gate",
      () => (action === "open" ? openGate(gateId, trimmed) : closeGate(gateId, trimmed)),
    );
    app.setStatus(response.message || `Gate ${action === "open" ? "opened" : "closed"}`);
    await Promise.all([fetchWorkflowRunDetail(runId, true), refreshWorkflowRunGates(runId, true)]);
  }

  function applyWorkflowRunDetail(
    detail: WorkflowRunDetail | null,
    metadata:
      { source: "http"; requestStartedVersion: number; requestId: number } | { source: "ws" } = {
      source: "ws",
    },
  ) {
    if (detail && metadata.source === "http") {
      const latestPushVersion = latestWorkflowRunPushVersion.get(detail.run.id) ?? 0;
      const latestRequestId = latestWorkflowRunHttpRequest.get(detail.run.id) ?? 0;

      if (
        latestPushVersion > metadata.requestStartedVersion ||
        latestRequestId !== metadata.requestId
      ) {
        console.info("[command-center] dropped stale workflow run detail", {
          runId: detail.run.id,
        });
        return;
      }
    }

    if (detail) {
      confirmPendingBreakpointPatch(detail);
    }

    if (detail) {
      runDetailById.set(detail.run.id, detail);

      if (!openRunIds.value.includes(detail.run.id)) {
        openRunIds.value = [...openRunIds.value, detail.run.id].slice(-MAX_OPEN_RUN_TABS);
      }

      selectedWorkflowRunId.value ??= detail.run.id;
    }

    const isActiveRun = detail ? detail.run.id === selectedWorkflowRunId.value : true;

    if (isActiveRun) {
      workflowRunDetail.value = detail;
      reapplyPendingBreakpointPatch();
      workflowNodeDetailExtra.value = "";

      if (!detail?.nodes.some((node) => node.node_id === selectedWorkflowRunNodeId.value)) {
        selectedWorkflowRunNodeId.value = detail?.nodes[0]?.node_id ?? "";
      }

      if (detail) {
        void syncWorkflowRunGatesForDetail(detail);
      } else {
        clearWorkflowRunGates();
      }
    }

    if (detail) {
      const resources = useResourcesStore();
      const hasWaiting = detail.nodes.some(
        (n) => n.status === "waiting" || n.status === "approval_required" || n.status === "pending",
      );

      if (hasWaiting) {
        void resources.refreshResources();
      }
    }
  }

  function reapplyPendingBreakpointPatch() {
    if (!workflowRunDetail.value || !pendingBreakpointPatch) {
      return;
    }

    if (workflowRunDetail.value.run.id !== pendingBreakpointPatch.runId) {
      return;
    }

    applyBreakpointPatch(workflowRunDetail.value, pendingBreakpointPatch.breakpoints);
  }

  function confirmPendingBreakpointPatch(detail: WorkflowRunDetail) {
    const pending = pendingBreakpointPatch;

    if (pending?.runId !== detail.run.id) {
      return;
    }

    if (sameBreakpoints(readBreakpoints(detail), pending.breakpoints)) {
      pendingBreakpointPatch = null;
    }
  }

  function clearPendingBreakpointPatch(runId: string, mutationId: number) {
    if (
      pendingBreakpointPatch?.runId === runId &&
      pendingBreakpointPatch.mutationId === mutationId
    ) {
      pendingBreakpointPatch = null;
      return true;
    }

    return false;
  }

  function applyBreakpointPatch(
    detail: { run: { state?: JsonRecord } } | null,
    breakpoints: string[],
  ) {
    if (!detail?.run.state) {
      return;
    }

    const debug = coerceDebugFrame(detail.run.state.debug) ?? {};
    detail.run.state.debug = { ...debug, breakpoints: [...breakpoints] };
  }

  function readBreakpoints(detail: { run: { state?: JsonRecord } }): string[] {
    return coerceDebugFrame(detail.run.state?.debug)?.breakpoints ?? [];
  }

  function sameBreakpoints(left: string[], right: string[]) {
    const normalizedLeft = [...new Set(left)].sort();
    const normalizedRight = [...new Set(right)].sort();

    if (normalizedLeft.length !== normalizedRight.length) {
      return false;
    }

    return normalizedLeft.every((id, index) => id === normalizedRight[index]);
  }

  function addWorkflowStep() {
    addWorkflowNode("action");
  }

  function addWorkflowNode(kind: WorkflowNodeKind) {
    const nodes = ensureWorkflowNodes();
    const newNode = createWorkflowNode(kind, nodes);
    stripNewNodeConnections(newNode);
    const position = graphCentroidPosition();
    const endIndex = nodes.findIndex((node: JsonRecord) => node.kind === "end");

    if (endIndex >= 0) {
      nodes.splice(endIndex, 0, newNode);
    } else {
      nodes.push(newNode);
    }

    setGraphNodePosition(displayValue(newNode.id), position);
    syncWorkflowDraftToJson();
    populateStepEditor(displayValue(newNode.id));
    openStepEditor(displayValue(newNode.id), true);
  }

  function addConnectedWorkflowNode(kind: WorkflowNodeKind = "action") {
    addWorkflowNode(kind);
  }

  function removeWorkflowStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) {
      return;
    }

    removeWorkflowNode(selectedStepId.value);
  }

  function removeWorkflowNode(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node || isLockedWorkflowNode(node)) {
      return;
    }

    workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
      (item: JsonRecord) => item.id !== nodeId,
    );
    removeWorkflowNodeReferences(workflowDraft.definition, nodeId);
    const layout = asRecord(asRecord(workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = Object.fromEntries(
      Object.entries(layoutNodes).filter(([entryId]) => entryId !== nodeId),
    );

    if (selectedStepId.value === nodeId) {
      selectedStepId.value = "";
    }

    syncWorkflowDraftToJson();
  }

  function applyInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    const previousId = nodeId;
    const result = applyWorkflowInlineNodeEdit(
      workflowDraft.definition,
      nodeId,
      nextId,
      inlineValue,
    );

    if (!result.ok) {
      app.setError(result.message);
      return false;
    }

    if (previousId !== result.nodeId) {
      renameLayoutNode(previousId, result.nodeId);
    }

    selectedStepId.value = result.nodeId;
    syncWorkflowDraftToJson();
    populateStepEditor(result.nodeId);
    return true;
  }

  function clearWorkflowGraphSelection() {
    selectedStepId.value = "";
    inlineEditNodeId.value = "";
    selectedGraphEdgeId.value = "";
  }

  function submitInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    if (!applyInlineNodeEdit(nodeId, nextId, inlineValue)) {
      return false;
    }

    clearWorkflowGraphSelection();
    return true;
  }

  function applyStepEditor(): boolean {
    if (stepEditorApplyTimer) {
      clearTimeout(stepEditorApplyTimer);
      stepEditorApplyTimer = null;
    }

    stepEditorError.value = "";

    if (!selectedStepId.value) {
      return false;
    }

    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === selectedStepId.value);

    if (index < 0) {
      return false;
    }

    if (isLockedWorkflowNode(nodes[index]) && stepEditor.kind !== nodes[index].kind) {
      const message = `${String(nodes[index].kind)} node kind cannot be changed`;
      stepEditorError.value = message;
      app.setError(message);
      return false;
    }

    const parameters = parseRequiredObject(stepEditor.parameters_json);
    const transitions = parseRequiredObject(stepEditor.transitions_json);

    if (!parameters || !transitions) {
      const message = parameters
        ? "Node transitions must be a JSON object"
        : "Step parameters must be a JSON object";
      stepEditorError.value = message;
      app.setError(message);
      return false;
    }

    const parameterError = validateStepParameters(parameters);

    if (parameterError) {
      stepEditorError.value = parameterError;
      app.setError(parameterError);
      return false;
    }

    // named slots so flow analysis tracks parameters/transitions/action edits
    // that a bare JsonRecord index signature would keep as `unknown`.
    type EditableNode = JsonRecord & {
      id?: string;
      action?: JsonRecord;
      parameters?: JsonRecord;
      transitions?: JsonRecord;
      wait?: JsonRecord;
      retry?: JsonRecord;
    };
    const next = { ...nodes[index] } as EditableNode;
    next.id = stepEditor.id.trim();

    if (!next.id) {
      stepEditorError.value = "Step ID is required";
      return false;
    }

    const trimmedName = stepEditor.name.trim();

    if (trimmedName) {
      next.name = trimmedName;
    } else {
      delete next.name;
    }

    next.kind = stepEditor.kind;

    if (next.kind === "action") {
      const previousAction = isRecord(next.action) ? next.action : {};
      next.action = {
        ...previousAction,
        provider: stepEditor.action_name,
        function: stepEditor.action_function,
        timeout_seconds:
          stepEditor.timeout_seconds > 0
            ? stepEditor.timeout_seconds
            : (previousAction.timeout_seconds ?? 300),
        configuration: parameters,
      };
    } else {
      delete next.action;
    }

    next.retry = { max_attempts: stepEditor.max_attempts };

    if (stepEditor.timeout_seconds > 0) {
      next.timeout_seconds = stepEditor.timeout_seconds;
    } else {
      delete next.timeout_seconds;
    }

    if (isProtectedWorkflowNode(next)) {
      delete next.locked;
    } else if (stepEditor.locked) {
      next.locked = true;
    } else {
      delete next.locked;
    }

    if (stepEditor.skipped) {
      next.skipped = true;
    } else {
      delete next.skipped;
    }

    // action nodes store inputs in action.configuration (set above); keep node.parameters clear to avoid duplication.
    next.parameters = next.kind === "action" ? {} : parameters;
    next.transitions = transitions;

    if (next.kind === "approval") {
      next.parameters = {
        ...parameters,
        approval_type: stepEditor.approval_type || "generic",
        prompt: stepEditor.approval_prompt || "Approval required",
      };
    }

    if (next.kind === "gate") {
      next.parameters = { ...parameters, kind: stepEditor.gate_kind || "manual" };

      if (stepEditor.gate_kind === "condition") {
        const when = parseRequiredObject(stepEditor.gate_when_json);

        if (!when) {
          stepEditorError.value = "Gate condition must be a JSON object";
          app.setError(stepEditorError.value);
          return false;
        }

        next.parameters.when = when;
      } else {
        delete next.parameters.when;
      }

      const pollInterval = stepEditor.gate_poll_interval;

      if (pollInterval > 0) {
        next.parameters.poll_interval = pollInterval;
      } else {
        delete next.parameters.poll_interval;
      }

      const timeout = stepEditor.gate_timeout;

      if (timeout > 0) {
        next.parameters.timeout = timeout;
      } else {
        delete next.parameters.timeout;
      }

      if (stepEditor.gate_label.trim()) {
        next.parameters.label = stepEditor.gate_label.trim();
      } else {
        delete next.parameters.label;
      }
    }

    if (next.kind === "signal") {
      next.parameters = { ...parameters, name: stepEditor.signal_name.trim() || "signal" };
    }

    if (next.kind === "condition") {
      next.transitions = { ...transitions, branches: [] };

      for (const [branchIndex, branch] of stepEditor.condition_branches.entries()) {
        const when = parseRequiredObject(branch.when_json);

        if (!when) {
          stepEditorError.value = `Condition branch ${String(branchIndex + 1)} must be a JSON object`;
          app.setError(stepEditorError.value);
          return false;
        }

        if (!branch.target) {
          stepEditorError.value = `Condition branch ${String(branchIndex + 1)} needs a target`;
          app.setError(stepEditorError.value);
          return false;
        }

        setConditionBranch(next, branchIndex, when, branch.target);
      }

      if (stepEditor.condition_fallback) {
        next.transitions.next = nodeRef(stepEditor.condition_fallback);
      } else {
        delete next.transitions.next;
      }
    }

    if (next.kind === "wait") {
      const wait = parseRequiredObject(stepEditor.wait_json);

      if (!wait) {
        stepEditorError.value = "Wait settings must be a JSON object";
        app.setError(stepEditorError.value);
        return false;
      }

      next.wait = {
        ...wait,
        seconds: Math.max(0, stepEditor.wait_seconds),
      };

      if (stepEditor.wait_initial_status.trim()) {
        next.wait.initial_status = stepEditor.wait_initial_status.trim();
      } else {
        delete next.wait.initial_status;
      }

      if (stepEditor.wait_until_status.trim()) {
        next.wait.until_status = stepEditor.wait_until_status.trim();
      } else {
        delete next.wait.until_status;
      }
    } else {
      delete next.wait;
    }

    if (next.kind === "loop") {
      const items = parseStepJson("Loop items", stepEditor.loop_items_json);

      if (!items.ok) {
        return false;
      }

      next.parameters = { ...parameters, items: items.value };

      if (stepEditor.loop_target) {
        next.parameters.target = nodeRef(stepEditor.loop_target);
      } else {
        delete next.parameters.target;
      }

      next.max_iterations = Math.max(1, stepEditor.loop_max_iterations);
    } else {
      delete next.max_iterations;
    }

    if (next.kind === "switch") {
      const value = parseStepJson("Switch value", stepEditor.switch_value_json);

      if (!value.ok) {
        return false;
      }

      const cases: JsonRecord[] = [];

      for (const [caseIndex, switchCase] of stepEditor.switch_cases.entries()) {
        if (!switchCase.target) {
          setStepEditorError(`Switch case ${String(caseIndex + 1)} needs a target`);
          return false;
        }

        const match = parseStepJson(`Switch case ${String(caseIndex + 1)}`, switchCase.match_json);

        if (!match.ok) {
          return false;
        }

        const serialized: JsonRecord = { target: nodeRef(switchCase.target) };

        if (switchCase.match_kind === "when") {
          serialized.when = match.value;
        } else if (switchCase.match_kind === "exists") {
          serialized.exists = Boolean(match.value);
        } else {
          serialized[switchCase.match_kind] = match.value;
        }

        cases.push(serialized);
      }

      next.parameters = { ...parameters, value: value.value, cases };

      if (stepEditor.switch_default) {
        next.parameters.default = nodeRef(stepEditor.switch_default);
      } else {
        delete next.parameters.default;
      }
    }

    if (next.kind === "toggle") {
      const value = parseStepJson("Toggle value", stepEditor.toggle_value_json);

      if (!value.ok) {
        return false;
      }

      if (!stepEditor.toggle_on || !stepEditor.toggle_off) {
        setStepEditorError("Toggle needs both an on and an off target");
        return false;
      }

      next.parameters = {
        ...parameters,
        value: value.value,
        on: nodeRef(stepEditor.toggle_on),
        off: nodeRef(stepEditor.toggle_off),
      };
    }

    if (next.kind === "percentage") {
      const key = parseStepJson("Percentage key", stepEditor.percentage_key_json);

      if (!key.ok) {
        return false;
      }

      const buckets: JsonRecord[] = [];

      for (const [bucketIndex, bucket] of stepEditor.percentage_buckets.entries()) {
        if (!bucket.target) {
          setStepEditorError(`Bucket ${String(bucketIndex + 1)} needs a target`);
          return false;
        }

        const weight = Math.trunc(bucket.weight);

        if (!Number.isFinite(weight) || weight <= 0) {
          setStepEditorError(`Bucket ${String(bucketIndex + 1)} needs a weight greater than zero`);
          return false;
        }

        buckets.push({ weight, target: nodeRef(bucket.target) });
      }

      next.parameters = { ...parameters, key: key.value, buckets };

      if (stepEditor.percentage_default) {
        next.parameters.default = nodeRef(stepEditor.percentage_default);
      } else {
        delete next.parameters.default;
      }
    }

    if (next.kind === "parallel") {
      next.parameters = {
        ...parameters,
        branches: stepEditor.parallel_branches.filter(Boolean).map(nodeRef),
      };
    }

    if (next.kind === "join") {
      next.parameters = {
        ...parameters,
        wait_for: stepEditor.join_wait_for.filter(Boolean).map(nodeRef),
        mode: stepEditor.join_mode,
      };
    }

    if (next.kind === "try") {
      next.parameters = { ...parameters };

      if (stepEditor.try_body) {
        next.parameters.body = nodeRef(stepEditor.try_body);
      } else {
        delete next.parameters.body;
      }

      if (stepEditor.try_catch) {
        next.parameters.catch = nodeRef(stepEditor.try_catch);
      } else {
        delete next.parameters.catch;
      }

      if (stepEditor.try_finally) {
        next.parameters.finally = nodeRef(stepEditor.try_finally);
      } else {
        delete next.parameters.finally;
      }
    }

    if (next.kind === "map") {
      const items = parseStepJson("Map items", stepEditor.map_items_json);

      if (!items.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        items: items.value,
        concurrency: Math.max(1, stepEditor.map_concurrency),
      };

      if (stepEditor.map_target) {
        next.parameters.target = nodeRef(stepEditor.map_target);
      } else {
        delete next.parameters.target;
      }
    }

    if (next.kind === "race") {
      next.parameters = {
        ...parameters,
        branches: stepEditor.race_branches.filter(Boolean).map(nodeRef),
        winner: stepEditor.race_winner,
      };
    }

    if (next.kind === "output") {
      const data = parseStepJson("Output data", stepEditor.output_data_json);

      if (!data.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        event_type: stepEditor.output_event_type.trim() || "workflow.output",
        data: data.value,
      };
    }

    if (next.kind === "input") {
      next.parameters = {
        ...parameters,
        prompt: stepEditor.input_prompt.trim() || "Provide input",
      };
    }

    if (next.kind === "config") {
      const name = parseStepJson("Config name", stepEditor.config_name_json);

      if (!name.ok) {
        return false;
      }

      const metadata = parseStepJson("Config metadata", stepEditor.config_metadata_json);

      if (!metadata.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        name: name.value,
        metadata: metadata.value,
      };
    }

    if (next.kind === "subflow") {
      const subflowParameters = parseRequiredObject(stepEditor.subflow_parameters_json);

      if (!subflowParameters) {
        setStepEditorError("Subflow parameters must be a JSON object");
        return false;
      }

      if (!stepEditor.subflow_id.trim()) {
        setStepEditorError("Subflow workflow id is required");
        return false;
      }

      next.subflow_id = stepEditor.subflow_id.trim();
      next.parameters = subflowParameters;
    } else {
      delete next.subflow_id;
    }

    if (next.kind === "assert") {
      const assertions: JsonRecord[] = [];

      for (const [assertIndex, assertion] of stepEditor.assert_assertions.entries()) {
        const condition = parseStepJson(
          `Assertion ${String(assertIndex + 1)} condition`,
          assertion.condition_json,
        );

        if (!condition.ok) {
          return false;
        }

        const serialized: JsonRecord = { condition: condition.value };

        if (assertion.name.trim()) {
          serialized.name = assertion.name.trim();
        }

        if (assertion.message.trim()) {
          serialized.message = assertion.message.trim();
        }

        assertions.push(serialized);
      }

      next.parameters = { ...parameters, assertions };
    }

    if (next.kind === "transform") {
      const bindings = parseRequiredObject(stepEditor.transform_bindings_json);

      if (!bindings) {
        setStepEditorError("Transform bindings must be a JSON object");
        return false;
      }

      next.parameters = { ...parameters, bindings };
    }

    if (next.kind === "audit") {
      const action = parseStepJson("Audit action", stepEditor.audit_action_json);

      if (!action.ok) {
        return false;
      }

      next.parameters = { ...parameters, action: action.value };
      const optionalAudit: JsonRecord = {};

      for (const [field, text] of [
        ["actor", stepEditor.audit_actor_json],
        ["target", stepEditor.audit_target_json],
        ["reason", stepEditor.audit_reason_json],
      ] as const) {
        const parsed = parseOptionalExpr(`Audit ${field}`, text);

        if (!parsed.ok) {
          return false;
        }

        if (parsed.value !== undefined) {
          optionalAudit[field] = parsed.value;
        }
      }

      next.parameters = { ...next.parameters, ...optionalAudit };
    }

    if (next.kind === "checkpoint") {
      if (!stepEditor.checkpoint_name.trim()) {
        setStepEditorError("Checkpoint needs a name");
        return false;
      }

      next.parameters = { ...parameters, name: stepEditor.checkpoint_name.trim() };
    }

    if (next.kind === "mutex") {
      if (!stepEditor.mutex_name.trim()) {
        setStepEditorError("Mutex needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.mutex_name.trim(),
        poll_interval_seconds: Math.max(1, stepEditor.mutex_poll_interval),
      };
    }

    if (next.kind === "throttle") {
      if (!stepEditor.throttle_name.trim()) {
        setStepEditorError("Throttle needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.throttle_name.trim(),
        max_per_window: Math.max(1, stepEditor.throttle_max_per_window),
        window_seconds: Math.max(1, stepEditor.throttle_window_seconds),
        poll_interval_seconds: Math.max(1, stepEditor.throttle_poll_interval),
      };
    }

    if (next.kind === "await_run") {
      const runIds = parseStepJson("Await run ids", stepEditor.await_run_ids_json);

      if (!runIds.ok) {
        return false;
      }

      next.parameters = {
        ...parameters,
        run_ids: runIds.value,
        mode: stepEditor.await_mode === "any" ? "any" : "all",
        poll_interval_seconds: Math.max(1, stepEditor.await_poll_interval),
      };
    }

    if (next.kind === "debounce") {
      if (!stepEditor.debounce_name.trim()) {
        setStepEditorError("Debounce needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.debounce_name.trim(),
        delay_seconds: Math.max(1, stepEditor.debounce_delay_seconds),
      };
      const triggerKey = parseOptionalExpr(
        "Debounce trigger key",
        stepEditor.debounce_trigger_key_json,
      );

      if (!triggerKey.ok) {
        return false;
      }

      if (triggerKey.value === undefined) {
        delete next.parameters.trigger_key;
      } else {
        next.parameters.trigger_key = triggerKey.value;
      }
    }

    if (next.kind === "collect") {
      if (!stepEditor.collect_name.trim()) {
        setStepEditorError("Collect needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.collect_name.trim(),
        max: Math.max(1, stepEditor.collect_max),
      };
    }

    if (next.kind === "barrier") {
      if (!stepEditor.barrier_name.trim()) {
        setStepEditorError("Barrier needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.barrier_name.trim(),
        count: Math.max(1, stepEditor.barrier_count),
        poll_interval_seconds: Math.max(1, stepEditor.barrier_poll_interval),
      };
    }

    if (next.kind === "circuit_breaker") {
      if (!stepEditor.circuit_name.trim()) {
        setStepEditorError("Circuit breaker needs a name");
        return false;
      }

      next.parameters = {
        ...parameters,
        name: stepEditor.circuit_name.trim(),
        threshold: Math.max(1, stepEditor.circuit_threshold),
        window_seconds: Math.max(1, stepEditor.circuit_window_seconds),
        cooldown_seconds: Math.max(0, stepEditor.circuit_cooldown_seconds),
      };
    }

    if (next.kind === "event_source") {
      next.parameters = { ...parameters, event_type: stepEditor.event_source_type.trim() || "*" };
      const filter = parseOptionalExpr("Event source filter", stepEditor.event_source_filter_json);

      if (!filter.ok) {
        return false;
      }

      if (filter.value === undefined) {
        delete next.parameters.filter;
      } else {
        next.parameters.filter = filter.value;
      }

      const max = Math.trunc(stepEditor.event_source_max);

      if (Number.isFinite(max) && max > 0) {
        next.parameters.max = max;
      } else {
        delete next.parameters.max;
      }
    }

    nodes[index] = next;

    if (selectedStepId.value !== next.id) {
      renameLayoutNode(selectedStepId.value, next.id);
    }

    selectedStepId.value = next.id;
    syncWorkflowDraftToJson();
    return true;
  }

  function populateStepEditor(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);

    if (!node) {
      return;
    }

    const parameters = asRecord(node.parameters);
    const transitions = asRecord(node.transitions);
    const wait = asRecord(node.wait);
    const retry = asRecord(node.retry);
    stepEditorHydrating = true;

    if (stepEditorApplyTimer) {
      clearTimeout(stepEditorApplyTimer);
      stepEditorApplyTimer = null;
    }

    selectedStepId.value = nodeId;
    stepEditor.id = nodeId;
    stepEditor.name = displayValue(node.name);
    stepEditor.kind = displayValue(node.kind) || "action";
    stepEditor.approval_type = displayValue(parameters.approval_type) || "generic";
    stepEditor.approval_prompt = displayValue(parameters.prompt) || "Approval required";
    stepEditor.gate_kind = displayValue(parameters.kind) || "manual";
    stepEditor.gate_when_json = pretty(parameters.when ?? {});
    stepEditor.gate_poll_interval = Number(parameters.poll_interval ?? 30);
    stepEditor.gate_timeout = Number(parameters.timeout ?? 0);
    stepEditor.gate_label = displayValue(parameters.label);
    stepEditor.signal_name = displayValue(parameters.name) || "signal";
    stepEditor.condition_fallback = nodeRefId(transitions.next) ?? "";
    stepEditor.condition_branches = asArray(transitions.branches).map((branch) => {
      const record = asRecord(branch);
      return { when_json: pretty(record.when ?? {}), target: nodeRefId(record.target) ?? "" };
    });
    stepEditor.wait_seconds = Number(wait.seconds ?? 60);
    stepEditor.wait_initial_status = displayValue(wait.initial_status) || "waiting";
    stepEditor.wait_until_status = displayValue(wait.until_status);
    stepEditor.wait_json = pretty(node.wait ?? {});
    stepEditor.loop_items_json = pretty(parameters.items ?? []);
    stepEditor.loop_target = nodeRefId(parameters.target) ?? "";
    stepEditor.loop_max_iterations = Number(node.max_iterations ?? 10);
    stepEditor.switch_value_json = pretty(parameters.value ?? valueRef("params", ["mode"]));
    stepEditor.switch_cases = asArray(parameters.cases).map((value) =>
      switchCaseEditor(asRecord(value)),
    );
    stepEditor.switch_default = nodeRefId(parameters.default) ?? "";
    stepEditor.toggle_value_json = pretty(
      parameters.value ?? valueRef("config", ["flags", "enabled"]),
    );
    stepEditor.toggle_on = nodeRefId(parameters.on) ?? "";
    stepEditor.toggle_off = nodeRefId(parameters.off) ?? "";
    stepEditor.percentage_key_json = pretty(parameters.key ?? valueRef("input", ["user_id"]));
    stepEditor.percentage_buckets = asArray(parameters.buckets).map((bucket) => {
      const record = asRecord(bucket);
      return { weight: Number(record.weight ?? 0), target: nodeRefId(record.target) ?? "" };
    });
    stepEditor.percentage_default = nodeRefId(parameters.default) ?? "";
    stepEditor.parallel_branches = nodeRefArray(parameters.branches);
    stepEditor.join_wait_for = nodeRefArray(parameters.wait_for);
    stepEditor.join_mode = branchPolicyName(parameters.mode, "all");
    stepEditor.try_body = nodeRefId(parameters.body) ?? "";
    stepEditor.try_catch = nodeRefId(parameters.catch) ?? "";
    stepEditor.try_finally = nodeRefId(parameters.finally) ?? "";
    stepEditor.map_items_json = pretty(parameters.items ?? []);
    stepEditor.map_target = nodeRefId(parameters.target) ?? "";
    stepEditor.map_concurrency = Number(parameters.concurrency ?? 1);
    stepEditor.race_branches = nodeRefArray(parameters.branches);
    stepEditor.race_winner = branchPolicyName(parameters.winner, "first_success");
    stepEditor.output_event_type = displayValue(parameters.event_type) || "workflow.output";
    stepEditor.output_data_json = stepEditorJson(parameters.data ?? null);
    stepEditor.input_prompt = displayValue(parameters.prompt) || "Provide input";
    stepEditor.config_name_json = stepEditorJson(parameters.name ?? "");
    stepEditor.config_metadata_json = stepEditorJson(parameters.metadata ?? {});
    stepEditor.subflow_id = displayValue(node.subflow_id);
    stepEditor.subflow_parameters_json = pretty(node.parameters ?? {});
    stepEditor.assert_assertions = asArray(parameters.assertions).map((assertion) => {
      const record = asRecord(assertion);
      return {
        name: displayValue(record.name),
        condition_json: pretty(record.condition ?? true),
        message: displayValue(record.message),
      };
    });
    stepEditor.transform_bindings_json = pretty(parameters.bindings ?? {});
    stepEditor.audit_action_json = stepEditorJson(parameters.action ?? "workflow.audit");
    stepEditor.audit_actor_json = optionalExprJson(parameters.actor);
    stepEditor.audit_target_json = optionalExprJson(parameters.target);
    stepEditor.audit_reason_json = optionalExprJson(parameters.reason);
    stepEditor.checkpoint_name = displayValue(parameters.name);
    stepEditor.mutex_name = displayValue(parameters.name);
    stepEditor.mutex_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    stepEditor.throttle_name = displayValue(parameters.name);
    stepEditor.throttle_max_per_window = Number(parameters.max_per_window ?? 10);
    stepEditor.throttle_window_seconds = Number(parameters.window_seconds ?? 60);
    stepEditor.throttle_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    stepEditor.await_run_ids_json = pretty(parameters.run_ids ?? valueRef("params", ["run_ids"]));
    stepEditor.await_mode = parameters.mode === "any" ? "any" : "all";
    stepEditor.await_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    stepEditor.debounce_name = displayValue(parameters.name);
    stepEditor.debounce_delay_seconds = Number(parameters.delay_seconds ?? 30);
    stepEditor.debounce_trigger_key_json = optionalExprJson(parameters.trigger_key);
    stepEditor.collect_name = displayValue(parameters.name);
    stepEditor.collect_max = Number(parameters.max ?? 10);
    stepEditor.barrier_name = displayValue(parameters.name);
    stepEditor.barrier_count = Number(parameters.count ?? 2);
    stepEditor.barrier_poll_interval = Number(parameters.poll_interval_seconds ?? 30);
    stepEditor.circuit_name = displayValue(parameters.name);
    stepEditor.circuit_threshold = Number(parameters.threshold ?? 5);
    stepEditor.circuit_window_seconds = Number(parameters.window_seconds ?? 60);
    stepEditor.circuit_cooldown_seconds = Number(parameters.cooldown_seconds ?? 60);
    stepEditor.event_source_type = displayValue(parameters.event_type) || "*";
    stepEditor.event_source_filter_json = optionalExprJson(parameters.filter);
    stepEditor.event_source_max = Number(parameters.max ?? 0);
    stepEditor.locked = isLockedWorkflowNode(node);
    stepEditor.skipped = node.skipped === true;
    stepEditor.max_attempts = Number(retry.max_attempts ?? 1);
    stepEditor.timeout_seconds = Number(node.timeout_seconds ?? 0);
    const actionConfig = workflowNodeActionConfig(node);
    stepEditor.action_name = actionConfig.provider;
    stepEditor.action_function = actionConfig.action;
    // action nodes carry their inputs in action.configuration (merged with node.parameters); show the effective set.
    const actionInputs =
      node.kind === "action" ? workflowNodeActionInputs(node) : (node.parameters ?? {});
    stepEditor.parameters_json = pretty(actionInputs);
    stepEditor.transitions_json = pretty(node.transitions ?? {});
    workflowInspectorMode.value = "step";
    void updateSelectedWorkflowNodeDetail();
    setTimeout(() => {
      stepEditorHydrating = false;
    }, 0);
  }

  async function updateSelectedWorkflowNodeDetail() {
    selectedWorkflowNodeRunId.value = null;
    workflowNodeDetailExtra.value = "";
    const nodeId = selectedWorkflowRunNodeId.value || selectedStepId.value;
    const step = workflowRunDetail.value?.nodes.find((node) => node.node_id === nodeId);

    if (!step?.id) {
      return;
    }

    selectedWorkflowNodeRunId.value = step.id;
    const [nodeChunks, nodeArtifacts] = await Promise.all([
      app
        .runOperation("Loading node chunks", () => fetchWorkflowNodeRunChunks(step.id))
        .catch(() => [] as RunChunk[]),
      app
        .runOperation("Loading node artifacts", () => fetchWorkflowNodeRunArtifacts(step.id))
        .catch(() => [] as RunArtifact[]),
    ]);
    workflowNodeDetailExtra.value = [
      "",
      `Workflow node run ${step.id} chunks`,
      ...nodeChunks.map((chunk) => `[${chunk.stream}] ${chunk.content}`),
      "",
      `Workflow node run ${step.id} artifacts`,
      ...nodeArtifacts.map(
        (artifact) => `${artifact.name} (${String(artifact.size_bytes)} bytes) ${artifact.uri}`,
      ),
    ].join("\n");
  }

  function workflowNameForRun(run: RunSummary): string {
    return workflows.value.find((workflow) => workflow.id === run.workflow_id)?.name ?? "";
  }

  function onGraphNodeClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (nodeId) {
      dismissStepEditorForCanvasEdit();
      selectedGraphEdgeId.value = "";
      // a single click only selects the node for the inspector/reference panel; it must not
      // drop the node into its inline mini-editor.
      inlineEditNodeId.value = "";
      populateStepEditor(nodeId);
    }
  }

  function onGraphNodeDoubleClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (!nodeId) {
      return;
    }

    // a double click opens the inline mini-editor on the node itself.
    selectedGraphEdgeId.value = "";
    populateStepEditor(nodeId);
    inlineEditNodeId.value = nodeId;
  }

  function onGraphNodeDragStop(event: NodeDragEvent) {
    const node = event.node;

    if (!node.id) {
      return;
    }

    dismissStepEditorForCanvasEdit();
    setGraphNodePosition(node.id, node.position);
    syncWorkflowDraftToJson();
  }

  function onGraphNodesChange(changes: NodeChange[]) {
    let changed = false;

    for (const change of changes) {
      if (change.type !== "position" || !change.id) {
        continue;
      }

      if (change.dragging) {
        continue;
      }

      setGraphNodePosition(change.id, change.position);
      changed = true;
    }

    if (changed) {
      syncWorkflowDraftToJson();
    }
  }

  function workflowEdgeOptions(sourceId: string) {
    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === sourceId);
    return sourceNode ? workflowEdgeSemanticOptions(sourceNode) : [];
  }

  function openEdgeEditorDraft(edgeId: string): WorkflowEdgeEditorDraft | null {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);
    return edge ? workflowEdgeEditorDraft(workflowDraft, edge) : null;
  }

  function selectGraphEdge(edgeId: string) {
    selectedStepId.value = "";
    selectedGraphEdgeId.value = edgeId;
  }

  function applyEdgeEditorDraft(draft: WorkflowEdgeEditorDraft): boolean {
    const previousEdge = draft.edgeId
      ? (graphEdges.value.find((edge: Edge) => edge.id === draft.edgeId) ?? null)
      : null;
    const result = applyWorkflowEdgeEditorDraft(workflowDraft.definition, previousEdge, draft);

    if (!result.ok) {
      app.setError(result.message);
      return false;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    return true;
  }

  function moveEdgeEditorItem(
    draft: WorkflowEdgeEditorDraft,
    direction: -1 | 1,
  ): WorkflowEdgeEditorDraft | null {
    const result = moveWorkflowEdgeEditorDraft(workflowDraft.definition, draft, direction);

    if (!result.ok) {
      app.setError(result.message);
      return null;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    const movedEdge = graphEdges.value.find(
      (edge: Edge) =>
        edge.source === result.draft.source &&
        edge.target === result.draft.target &&
        workflowEdgeOptionId(edge) === result.draft.optionId,
    );
    return movedEdge ? { ...result.draft, edgeId: movedEdge.id } : result.draft;
  }

  function moveSelectedEdge(direction: -1 | 1): boolean {
    const draft = selectedGraphEdgeId.value ? openEdgeEditorDraft(selectedGraphEdgeId.value) : null;

    if (!draft) {
      return false;
    }

    const moved = moveEdgeEditorItem(draft, direction);

    if (!moved) {
      return false;
    }

    selectedGraphEdgeId.value = moved.edgeId;
    return true;
  }

  function reverseSelectedEdgeHandles(): boolean {
    const edge = selectedGraphEdge.value;

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    const data = edge.data as WorkflowEditorEdgeData | undefined;
    const semanticKey =
      data?.transitionKey ??
      (typeof data?.branchIndex === "number"
        ? `branches.${String(data.branchIndex)}`
        : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    setWorkflowEdgeHandles(
      workflowDraft.definition,
      edge.source,
      semanticKey,
      edge.targetHandle,
      edge.sourceHandle,
      data?.edgeStyle,
    );
    syncWorkflowDraftToJson();
    selectedGraphEdgeId.value = "";
    return true;
  }

  function setEdgeLabelOffset(edgeId: string, offset: { x: number; y: number } | null): boolean {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelOffset(workflowDraft.definition, edge, offset);
    syncWorkflowDraftToJson();
    return true;
  }

  function setEdgeLabelAnchor(edgeId: string, position: number | null): boolean {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);

    if (!edge) {
      return false;
    }

    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelAnchor(
      workflowDraft.definition,
      edge,
      position === null ? null : { position },
    );
    syncWorkflowDraftToJson();
    return true;
  }

  function scheduleStepEditorApply() {
    void applyStepEditor();
  }

  function applyGraphEdgeSemantic(
    connection: Connection,
    optionId: string,
    previousEdgeId = "",
  ): boolean {
    const { source, target, sourceHandle } = connection;

    if (!source || !target) {
      return false;
    }

    dismissStepEditorForCanvasEdit();

    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return false;
    }

    const previousEdge = previousEdgeId
      ? (graphEdges.value.find((edge: Edge) => edge.id === previousEdgeId) ?? null)
      : null;
    const previousDraft = previousEdge
      ? workflowEdgeEditorDraft(workflowDraft, previousEdge)
      : null;
    const draft: WorkflowEdgeEditorDraft = {
      ...(previousDraft ?? defaultEdgeEditorDraft()),
      edgeId: previousEdgeId,
      source,
      target,
      optionId,
      sourceHandle,
      targetHandle: connection.targetHandle,
    };
    return applyEdgeEditorDraft(draft);
  }

  function onGraphConnect(connection: Connection) {
    const source = connection.source;
    const handleOptionId = optionIdForSourceHandle(connection.sourceHandle);
    const options = workflowEdgeOptions(source);

    if (!source || options.length === 0) {
      return;
    }

    const optionId =
      handleOptionId && options.some((option) => option.id === handleOptionId)
        ? handleOptionId
        : options.length === 1
          ? options[0].id
          : "";

    if (optionId) {
      applyGraphEdgeSemantic(connection, optionId);
    }
  }

  function onGraphEdgeClick(event: EdgeMouseEvent) {
    const edgeId = event.edge.id;

    if (edgeId) {
      selectGraphEdge(edgeId);
    }
  }

  function onGraphEdgeUpdate(event: EdgeUpdateEvent) {
    const edge = event.edge;
    const connection = event.connection;

    if (!connection.source || !connection.target) {
      return;
    }

    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return;
    }

    const optionId = workflowEdgeOptionId(edge);

    if (!optionId) {
      return;
    }

    if (
      applyGraphEdgeSemantic(connection, optionId, edge.id) &&
      selectedStepId.value === edge.source
    ) {
      populateStepEditor(edge.source);
    }

    selectedGraphEdgeId.value = "";
  }

  function onGraphEdgesChange(changes: EdgeChange[]) {
    let changed = false;

    for (const change of changes) {
      if (change.type === "remove") {
        const edge = graphEdges.value.find((item: Edge) => item.id === change.id);

        if (edge) {
          const sourceNode = ensureWorkflowNodes().find((n: JsonRecord) => n.id === edge.source);

          if (sourceNode && removeWorkflowEdge(sourceNode, edge)) {
            const data = edge.data as WorkflowEditorEdgeData | undefined;

            if (data?.transitionKey) {
              removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
            }

            if (typeof data?.branchIndex === "number") {
              removeWorkflowEdgeHandles(
                workflowDraft.definition,
                edge.source,
                `branches.${String(data.branchIndex)}`,
              );
            }

            if (data?.parameterKey) {
              removeWorkflowEdgeHandles(
                workflowDraft.definition,
                edge.source,
                parameterSemanticKey(data.parameterKey, data.parameterIndex),
              );
            }

            changed = true;
          }
        }
      }
    }

    if (changed) {
      syncWorkflowDraftToJson();

      if (selectedStepId.value) {
        populateStepEditor(selectedStepId.value);
      }
    }
  }

  function removeWorkflowEdgeById(edgeId: string) {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);

    if (!edge) {
      return;
    }

    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === edge.source);

    if (!sourceNode || !removeWorkflowEdge(sourceNode, edge)) {
      return;
    }

    const data = edge.data as WorkflowEditorEdgeData | undefined;

    if (data?.transitionKey) {
      removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
    }

    if (typeof data?.branchIndex === "number") {
      removeWorkflowEdgeHandles(
        workflowDraft.definition,
        edge.source,
        `branches.${String(data.branchIndex)}`,
      );
    }

    if (data?.parameterKey) {
      removeWorkflowEdgeHandles(
        workflowDraft.definition,
        edge.source,
        parameterSemanticKey(data.parameterKey, data.parameterIndex),
      );
    }

    syncWorkflowDraftToJson();

    if (selectedStepId.value) {
      populateStepEditor(selectedStepId.value);
    }
  }

  function autoArrangeWorkflowNodes(
    direction: WorkflowLayoutDirection = workflowLayoutDirection.value,
  ) {
    if (!syncWorkflowJson()) {
      return;
    }

    workflowLayoutDirection.value = direction;
    const positions = autoArrangeWorkflowLayout(workflowDraft.definition, direction);

    for (const [nodeId, position] of Object.entries(positions)) {
      setGraphNodePosition(nodeId, position);
    }

    autoArrangeWorkflowEdgeHandles(workflowDraft.definition, positions);
    workflowLayoutVersion.value += 1;
    syncWorkflowDraftToJson();
  }

  function scheduleWorkflowJsonSync() {
    void syncWorkflowJson();
  }

  function scheduleWorkflowWdlSync() {
    if (workflowWdlSyncTimer) {
      clearTimeout(workflowWdlSyncTimer);
    }

    workflowWdlSyncTimer = setTimeout(() => {
      workflowWdlSyncTimer = null;
      void syncWorkflowWdl();
    }, WORKFLOW_WDL_SYNC_DELAY_MS);
  }

  function scheduleWorkflowWdlRefresh() {
    void refreshWorkflowWdl();
  }

  function setWorkflowJsonSilently(next: string) {
    if (workflowJsonWriteReleaseTimer) {
      clearTimeout(workflowJsonWriteReleaseTimer);
    }

    workflowJsonWriteGuard = true;
    workflowJson.value = next;
    workflowJsonWriteReleaseTimer = setTimeout(() => {
      workflowJsonWriteGuard = false;
      workflowJsonWriteReleaseTimer = null;
    }, 0);
  }

  function setWorkflowWdlSilently(next: string) {
    if (workflowWdlWriteReleaseTimer) {
      clearTimeout(workflowWdlWriteReleaseTimer);
    }

    workflowWdlWriteGuard = true;
    workflowWdl.value = next;
    workflowWdlWriteReleaseTimer = setTimeout(() => {
      workflowWdlWriteGuard = false;
      workflowWdlWriteReleaseTimer = null;
    }, 0);
  }

  function syncWorkflowJson(): boolean {
    const parsed = parseRequiredObject(workflowJson.value);

    if (!parsed) {
      app.setError("Workflow definition must be a JSON object");
      return false;
    }

    const errors = validateWorkflowReferenceSyntax(parsed);

    if (errors.length > 0) {
      app.setError(errors[0]);
      return false;
    }

    workflowDraft.definition = parsed;
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    isDirty.value = true;
    scheduleWorkflowWdlRefresh();
    return true;
  }

  function syncWorkflowDraftToJson() {
    // a graph edit is now the source of truth, so save should serialize the draft, not recompile wdl.
    workflowEditorMode.value = "graph";
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    isDirty.value = true;
    scheduleWorkflowWdlRefresh();
  }

  // compile the wdl editor contents into the draft definition. mirrors syncWorkflowJson:
  // on a compile error we keep the wdl text and surface the message rather than clobbering.
  async function syncWorkflowWdl(): Promise<boolean> {
    if (workflowWdlSyncTimer) {
      clearTimeout(workflowWdlSyncTimer);
      workflowWdlSyncTimer = null;
    }

    let compiled: WorkflowDefinition;
    const previousUi = isJsonObject(workflowDraft.definition.ui)
      ? cloneJson(workflowDraft.definition.ui)
      : null;

    try {
      compiled = await compileWdl(workflowWdl.value, workflowDraft.enabled);
    } catch (err) {
      app.setError(`WDL compile error: ${errorMessage(err)}`);
      return false;
    }

    workflowDraft.name = compiled.name;
    workflowDraft.version = compiled.version;
    workflowDraft.input_type = compiled.input_type;
    workflowDraft.definition = compiled.definition;

    if (previousUi) {
      workflowDraft.definition.ui = previousUi;
    }

    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    isDirty.value = true;
    return true;
  }

  // decompile the current draft into wdl text for the wdl pane. some control shapes cannot be
  // recovered structurally (e.g. an incomplete graph); on failure the pane goes read-only and shows
  // the reason via workflowWdlError so editing it never compiles empty wdl back over the draft.
  async function refreshWorkflowWdl(): Promise<void> {
    try {
      setWorkflowWdlSilently(await decompileToWdl(cloneJson(workflowDraft)));
      workflowWdlError.value = "";
    } catch (err) {
      setWorkflowWdlSilently("");
      workflowWdlError.value = errorMessage(err);
    }
  }

  // export the current workflow draft as a .wdl source file the importer can re-ingest. the web
  // service decompiles the definition; the caller (this client) writes it to disk via a download.
  async function exportWorkflowWdl(): Promise<void> {
    try {
      const source = await decompileToWdl(cloneJson(workflowDraft));
      const name = (workflowDraft.name.trim() || "workflow");
      const fileName = `${name.replace(/[^a-z0-9._-]+/gi, "_")}.wdl`;
      downloadTextFile(fileName, source, "text/plain");
      app.setStatus(`Exported ${fileName}`);
    } catch (err) {
      app.setError(`Could not export this workflow as WDL (${errorMessage(err)}).`);
    }
  }

  // export every workflow as a .wdlp pack: a zip of one decompiled .wdl per workflow plus a
  // pack.wdlp manifest listing those files and the workflows' triggers, ready for the importer.
  async function exportWorkflowPack(): Promise<void> {
    const allWorkflows = workflows.value.filter(
      (workflow): workflow is WorkflowDefinition & { id: string } => workflow.id != null,
    );

    if (allWorkflows.length === 0) {
      app.setError("No workflows to export.");
      return;
    }

    await app.runOperation("Exporting workflow pack", async () => {
      const entries: ZipEntry[] = [];
      const manifestWorkflows: string[] = [];
      const triggers: WorkflowTrigger[] = [];
      const usedNames = new Set<string>();
      const skipped: string[] = [];

      for (const workflow of allWorkflows) {
        let source: string;

        try {
          source = await decompileToWdl(cloneJson(workflow));
        } catch {
          skipped.push(workflow.name || `workflow ${workflow.id}`);
          continue;
        }

        let slug = (workflow.name.trim() || `workflow-${workflow.id}`).replace(
          /[^a-z0-9._-]+/gi,
          "_",
        );

        while (usedNames.has(slug)) {
          slug = `${slug}_${workflow.id}`;
        }

        usedNames.add(slug);
        const fileName = `${slug}.wdl`;
        entries.push({ name: fileName, content: source });
        manifestWorkflows.push(fileName);
        triggers.push(...(await fetchWorkflowTriggers(workflow.id).catch(() => [])));
      }

      if (entries.length === 0) {
        throw new Error("no workflows could be decompiled to WDL");
      }

      const manifest = { version: 1, workflows: manifestWorkflows, triggers };
      entries.unshift({ name: "pack.wdlp", content: pretty(manifest) });
      downloadBlob("runinator-pack.zip", createZip(entries));
      const note = skipped.length
        ? ` (skipped ${String(skipped.length)} non-WDL: ${skipped.join(", ")})`
        : "";
      app.setStatus(`Exported ${String(entries.length - 1)} workflow(s) to runinator-pack.zip${note}`);
    });
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!Array.isArray(workflowDraft.definition.nodes)) {
      workflowDraft.definition.nodes = [];
    }

    return workflowDraft.definition.nodes as JsonRecord[];
  }

  function stripNewNodeConnections(node: JsonRecord) {
    const transitions = asRecord(node.transitions);
    const omitTransitionKeys = new Set<string>([...directTransitionKeys, "branches"]);
    node.transitions = Object.fromEntries(
      Object.entries(transitions).filter(([entryKey]) => !omitTransitionKeys.has(entryKey)),
    );

    const parameters = asRecord(node.parameters);
    const omitParameterKeys = new Set(["target", "default", "body", "catch", "finally"]);
    const cleanedParameters = Object.fromEntries(
      Object.entries(parameters).filter(([entryKey]) => !omitParameterKeys.has(entryKey)),
    );

    if (Array.isArray(parameters.cases)) {
      cleanedParameters.cases = [];
    }

    if (Array.isArray(parameters.branches)) {
      cleanedParameters.branches = [];
    }

    if (Array.isArray(parameters.wait_for)) {
      cleanedParameters.wait_for = [];
    }

    node.parameters = cleanedParameters;
  }

  function graphCentroidPosition(): { x: number; y: number } {
    const positioned = graphNodes.value
      .map((node) => ({
        x: node.position.x,
        y: node.position.y,
      }))
      .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));

    if (positioned.length === 0) {
      return nextNodePosition(1);
    }

    const totals = positioned.reduce(
      (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
      { x: 0, y: 0 },
    );
    return {
      x: Math.round(totals.x / positioned.length),
      y: Math.round(totals.y / positioned.length),
    };
  }

  function moveWorkflowSelection(delta: number) {
    const list = filteredWorkflows.value;

    if (list.length === 0) {
      return;
    }

    const current = list.findIndex((workflow) => workflow.id === selectedWorkflowId.value);
    void selectWorkflow(list[boundedIndex(current, delta, list.length)]);
  }

  function setGraphNodePosition(nodeId: string, position: { x: number; y: number }) {
    const definition = workflowDraft.definition;
    const ui = asRecord(definition.ui);
    definition.ui = ui;
    const layout = asRecord(ui.layout);
    ui.layout = layout;
    const layoutNodes = asRecord(layout.nodes);
    layout.nodes = layoutNodes;
    layoutNodes[nodeId] = { x: position.x, y: position.y };
  }

  function renameLayoutNode(previousId: string, nextId: string) {
    if (!previousId || previousId === nextId) {
      return;
    }

    const layout = asRecord(asRecord(workflowDraft.definition.ui).layout);
    const layoutNodes = asRecord(layout.nodes);

    if (!layoutNodes[previousId]) {
      return;
    }

    const { [previousId]: movedNode, ...remainingNodes } = layoutNodes;
    layout.nodes = { ...remainingNodes, [nextId]: movedNode };
  }

  function addConditionBranchEditor() {
    stepEditor.condition_branches.push({
      when_json: pretty({ value: valueRef("params", ["value"]), equals: true }),
      target: "",
    });
    markWorkflowDirty();
  }

  function removeConditionBranchEditor(index: number) {
    stepEditor.condition_branches.splice(index, 1);
    const node = selectedNode.value;

    if (node?.kind === "condition") {
      removeConditionBranch(node, index);
    }

    markWorkflowDirty();
  }

  function addSwitchCaseEditor() {
    stepEditor.switch_cases.push({ match_kind: "equals", match_json: pretty(true), target: "" });
    markWorkflowDirty();
  }

  function removeSwitchCaseEditor(index: number) {
    stepEditor.switch_cases.splice(index, 1);
    markWorkflowDirty();
  }

  function addAssertionEditor() {
    stepEditor.assert_assertions.push({
      name: "",
      condition_json: pretty({ value: valueRef("params", ["value"]), equals: true }),
      message: "",
    });
    markWorkflowDirty();
  }

  function removeAssertionEditor(index: number) {
    stepEditor.assert_assertions.splice(index, 1);
    markWorkflowDirty();
  }

  function addPercentageBucketEditor() {
    stepEditor.percentage_buckets.push({ weight: 50, target: "" });
    markWorkflowDirty();
  }

  function removePercentageBucketEditor(index: number) {
    stepEditor.percentage_buckets.splice(index, 1);
    markWorkflowDirty();
  }

  function addNodeRefEditor(list: string[]) {
    list.push("");
    markWorkflowDirty();
  }

  function removeNodeRefEditor(list: string[], index: number) {
    list.splice(index, 1);
    markWorkflowDirty();
  }

  function openWorkflowSettings() {
    workflowSettingsOpen.value = true;
    void refreshWorkflowTriggers();
  }

  function closeWorkflowSettings() {
    workflowSettingsOpen.value = false;
    closeTriggerEditor();
  }

  async function refreshWorkflowTriggers() {
    const workflowId = workflowDraft.id;

    if (!workflowId) {
      workflowTriggers.value = [];
      closeTriggerEditor();
      return;
    }

    workflowTriggers.value = (await app
      .runOperation("Loading workflow triggers", () => fetchWorkflowTriggers(workflowId))
      .catch(() => [])) as WorkflowTrigger[];
  }

  function clearWorkflowTriggerState() {
    workflowTriggers.value = [];
    closeTriggerEditor();
  }

  function addWorkflowTrigger(kind: WorkflowTriggerKind = "cron") {
    if (!workflowDraft.id) {
      return;
    }

    Object.assign(triggerDraft, newWorkflowTriggerDraft(workflowDraft.id, kind));
    triggerJson.configuration = pretty(triggerDraft.configuration);
    triggerJson.metadata = pretty(triggerDraft.metadata);
    triggerEditorCreating.value = true;
    triggerEditorError.value = "";
    triggerEditorOpen.value = true;
  }

  function editWorkflowTrigger(trigger: WorkflowTrigger) {
    Object.assign(triggerDraft, cloneJson(trigger));
    triggerDraft.next_execution = triggerDateForInput(trigger.next_execution);
    triggerDraft.blackout_start = triggerDateForInput(trigger.blackout_start);
    triggerDraft.blackout_end = triggerDateForInput(trigger.blackout_end);
    triggerJson.configuration = pretty(trigger.configuration);
    triggerJson.metadata = pretty(trigger.metadata);
    triggerEditorCreating.value = false;
    triggerEditorError.value = "";
    triggerEditorOpen.value = true;
  }

  function closeTriggerEditor() {
    triggerEditorOpen.value = false;
    triggerEditorCreating.value = false;
    triggerEditorError.value = "";
  }

  function setTriggerKind(kind: WorkflowTriggerKind) {
    triggerDraft.kind = kind;

    if (triggerEditorCreating.value) {
      triggerDraft.configuration = defaultTriggerConfiguration(kind);
      triggerJson.configuration = pretty(triggerDraft.configuration);
    }
  }

  async function submitWorkflowTrigger() {
    triggerEditorError.value = "";

    if (!workflowDraft.id) {
      return;
    }

    const configuration = parseRequiredObject(triggerJson.configuration);
    const metadata = parseRequiredObject(triggerJson.metadata);

    if (!configuration || !metadata) {
      triggerEditorError.value = configuration
        ? "Trigger metadata must be a JSON object"
        : "Trigger configuration must be a JSON object";
      app.setError(triggerEditorError.value);
      return;
    }

    const trigger: WorkflowTrigger = {
      ...cloneJson(triggerDraft),
      workflow_id: workflowDraft.id,
      configuration,
      metadata,
      next_execution: dateTimeLocalToIso(triggerDraft.next_execution),
      blackout_start: dateTimeLocalToIso(triggerDraft.blackout_start),
      blackout_end: dateTimeLocalToIso(triggerDraft.blackout_end),
    };
    const saved = await app.runOperation("Saving workflow trigger", () =>
      saveWorkflowTrigger(trigger, triggerEditorCreating.value),
    );
    app.setStatus(`Workflow trigger saved: ${saved.kind}`);
    closeTriggerEditor();
    await refreshWorkflowTriggers();
  }

  async function deleteSelectedWorkflowTrigger(trigger: WorkflowTrigger) {
    const triggerId = trigger.id;

    if (!triggerId) {
      return;
    }

    if (!window.confirm(`Delete ${trigger.kind} trigger ${triggerId}?`)) {
      return;
    }

    const response = await app.runOperation("Deleting workflow trigger", () =>
      deleteWorkflowTrigger(triggerId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to delete workflow trigger");
      return;
    }

    app.setStatus(response.message || "Workflow trigger deleted");

    if (triggerDraft.id === trigger.id) {
      closeTriggerEditor();
    }

    await refreshWorkflowTriggers();
  }

  function triggerCronSummary(trigger: WorkflowTrigger): string {
    const cron = trigger.configuration.cron;
    return typeof cron === "string" && cron.trim() ? cron : "";
  }

  function triggerDateForInput(value: string | null | undefined): string {
    if (!value) {
      return "";
    }

    const date = new Date(value);

    if (Number.isNaN(date.getTime())) {
      return "";
    }

    const offset = date.getTimezoneOffset() * 60000;
    return new Date(date.getTime() - offset).toISOString().slice(0, 16);
  }

  function markWorkflowDirty() {
    isDirty.value = true;
  }

  function openStepEditor(nodeId: string, creating = false) {
    stepEditorBaselineDefinition = creating ? null : cloneJson(workflowDraft.definition);
    populateStepEditor(nodeId);
    stepEditorCreating.value = creating;
    stepEditorCreatedNodeId.value = creating ? nodeId : "";
    stepEditorError.value = "";
    workflowInspectorMode.value = "step";
    // the full modal supersedes the inline mini-editor.
    inlineEditNodeId.value = "";
    stepEditorOpen.value = true;
  }

  async function submitStepEditor() {
    if (!applyStepEditor()) {
      return;
    }

    stepEditorOpen.value = false;
    stepEditorCreating.value = false;
    stepEditorCreatedNodeId.value = "";
    selectedStepId.value = "";
    inlineEditNodeId.value = "";
    // applying a step persists the workflow so canvas edits do not need a manual save.
    await saveSelectedWorkflowBundle();
  }

  // dismiss an open node editor when the user starts editing on the canvas instead.
  function dismissStepEditorForCanvasEdit() {
    if (!stepEditorOpen.value || stepEditorCreating.value) {
      return;
    }

    stepEditorOpen.value = false;
    stepEditorError.value = "";
  }

  function closeStepEditor() {
    if (stepEditorApplyTimer) {
      clearTimeout(stepEditorApplyTimer);
      stepEditorApplyTimer = null;
    }

    if (stepEditorCreating.value && stepEditorCreatedNodeId.value) {
      const nodeId = stepEditorCreatedNodeId.value;
      workflowDraft.definition.nodes = ensureWorkflowNodes().filter(
        (node: JsonRecord) => node.id !== nodeId,
      );
      syncWorkflowDraftToJson();
    } else if (stepEditorBaselineDefinition) {
      workflowDraft.definition = cloneJson(stepEditorBaselineDefinition);
      syncWorkflowDraftToJson();
    }

    selectedStepId.value = "";
    inlineEditNodeId.value = "";
    stepEditorOpen.value = false;
    stepEditorCreating.value = false;
    stepEditorCreatedNodeId.value = "";
    stepEditorError.value = "";
    stepEditorBaselineDefinition = null;
    stepEditorHydrating = false;
  }

  function duplicateSelectedStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) {
      return;
    }

    const nodes = ensureWorkflowNodes();
    const source = nodes.find((node: JsonRecord) => node.id === selectedStepId.value);

    if (!source) {
      return;
    }

    const copy = cloneJson(source);
    const copyId = uniqueWorkflowNodeId(nodes, `${String(source.id)}_copy`);
    copy.id = copyId;
    stripNewNodeConnections(copy);
    const position = graphCentroidPosition();
    nodes.push(copy);
    setGraphNodePosition(copyId, position);
    syncWorkflowDraftToJson();
    populateStepEditor(copyId);
    openStepEditor(copyId, true);
  }

  function setStepEditorError(message: string) {
    stepEditorError.value = message;
    app.setError(message);
  }

  function workflowSaveTriggers(workflowId: string | null | undefined): WorkflowTrigger[] {
    if (workflowId == null) {
      return [];
    }

    return workflowTriggers.value
      .filter((trigger) => trigger.workflow_id === workflowId)
      .map((trigger) => cloneJson(trigger));
  }

  async function workflowWdlSaveRequest(): Promise<WorkflowWdlSaveRequest> {
    const workflow = cloneJson(workflowDraft);
    const workflowId = workflow.id ?? null;
    const source = await decompileToWdl(workflow);
    const triggers = workflowId === null ? [] : workflowSaveTriggers(workflowId);
    const request: WorkflowWdlSaveRequest = {
      source,
      enabled: workflow.enabled,
      workflow_id: workflowId,
      triggers,
    };

    if (isJsonObject(workflow.definition.ui)) {
      request.ui = cloneJson(workflow.definition.ui);
    }

    return request;
  }

  function parseStepJson(
    label: string,
    text: string,
  ): { ok: true; value: JsonValue } | { ok: false } {
    const value = parseRequiredJson(text);

    if (value !== null || text.trim() === "null") {
      return { ok: true, value: value ?? null };
    }

    setStepEditorError(`${label} must be valid JSON`);
    return { ok: false };
  }

  function stepEditorJson(value: unknown): string {
    return JSON.stringify(value === undefined ? null : asJsonValue(value), null, 2);
  }

  // an optional expression parameter renders as an empty editor when absent so it stays omitted.
  function optionalExprJson(value: unknown): string {
    return value === undefined || value === null ? "" : pretty(asJsonValue(value));
  }

  // parse an optional expression editor: blank means omit, otherwise it must be valid json.
  function parseOptionalExpr(
    label: string,
    text: string,
  ): { ok: true; value: JsonValue | undefined } | { ok: false } {
    if (text.trim() === "") {
      return { ok: true, value: undefined };
    }

    const parsed = parseStepJson(label, text);
    return parsed.ok ? { ok: true, value: parsed.value } : { ok: false };
  }

  function isJsonObject(value: unknown): value is JsonRecord {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function validateStepParameters(parameters: JsonRecord): string {
    if (stepEditor.kind !== "action") {
      return "";
    }

    const provider = providerCatalog().find((item) => item.name === stepEditor.action_name);
    const action = provider?.actions.find(
      (item) => item.function_name === stepEditor.action_function,
    );

    if (!action) {
      return "Select a valid task provider action";
    }

    for (const parameter of action.parameters) {
      if (!parameter.required) {
        continue;
      }

      const value = parameters[parameter.name];

      if (isBlankValue(value)) {
        return `${parameter.label ?? parameter.name} is required`;
      }

      const typeError = validateJsonValueType(
        value,
        parameter.ty,
        parameter.label ?? parameter.name,
      );

      if (typeError) {
        return typeError;
      }
    }

    return "";
  }

  async function saveSelectedWorkflowBundle() {
    const synced =
      workflowEditorMode.value === "wdl" ? await syncWorkflowWdl() : syncWorkflowJson();

    if (!synced) {
      return;
    }

    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    const saved = await app.runOperation("Saving workflow", async () =>
      saveWorkflowWdl(await workflowWdlSaveRequest()),
    );
    const savedWorkflow = saved.workflows.at(0);

    if (savedWorkflow === undefined) {
      app.setError("Workflow bundle save returned no workflow");
      return;
    }

    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(savedWorkflow)));
    workflowTriggers.value = saved.triggers.filter(
      (trigger) => trigger.workflow_id === workflowDraft.id,
    );
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    scheduleWorkflowWdlRefresh();
    app.setStatus(`Workflow saved: ${savedWorkflow.name}`);
    isDirty.value = false;
    selectedWorkflowId.value = savedWorkflow.id;
    await refreshWorkflows();
  }

  async function deleteSelectedWorkflow() {
    const workflow = selectedWorkflow.value;

    if (!workflow?.id) {
      return;
    }

    if (
      !window.confirm(
        `Delete workflow "${workflow.name}"?\n\nThis permanently deletes the workflow along with ALL of its runs and their execution history. This cannot be undone.`,
      )
    ) {
      return;
    }

    const workflowId = workflow.id;
    const response = await app.runOperation(`Deleting workflow ${workflow.name}`, () =>
      deleteWorkflow(workflowId),
    );

    if (!response.success) {
      app.setError(response.message || "Failed to delete workflow");
      return;
    }

    app.setStatus(response.message || `Workflow deleted: ${workflow.name}`);
    closeWorkflowSettings();
    const deletedId = workflow.id;
    workflows.value = workflows.value.filter((item) => item.id !== deletedId);
    selectedWorkflowId.value = workflows.value[0]?.id ?? null;

    if (workflows.value[0]) {
      await selectWorkflow(workflows.value[0]);
    } else {
      Object.assign(workflowDraft, newWorkflowDraft());
      setWorkflowJsonSilently(pretty(workflowDraft.definition));
      setWorkflowWdlSilently("");
      workflowWdlError.value = "";
      workflowRuns.value = [];
      workflowRunDetail.value = null;
      selectedWorkflowRunId.value = null;
      isDirty.value = false;
    }
  }

  async function duplicateSelectedWorkflow(bump: "major" | "minor" | "patch" = "minor") {
    const workflow = selectedWorkflow.value;

    if (!workflow?.id) {
      return;
    }

    if (isDirty.value) {
      app.setError("Save or discard the current changes before duplicating this workflow.");
      return;
    }

    const workflowId = workflow.id;
    const copy = await app
      .runOperation(`Duplicating workflow ${workflow.name}`, () =>
        duplicateWorkflow(workflowId, bump),
      )
      .catch((error: unknown) => {
        app.setError(error instanceof Error ? error.message : "Failed to duplicate workflow");
        return null;
      });

    if (!copy) {
      return;
    }

    await refreshWorkflows();
    selectedWorkflowId.value = copy.id;
    await selectWorkflow(copy);
    app.setStatus(`Duplicated ${workflow.name} as v${copy.version}`);
  }

  return {
    recentWorkflowRuns,
    getTransition,
    setTransition,
    workflows,
    selectedWorkflowId,
    workflowDraft,
    workflowJson,
    workflowWdl,
    workflowWdlError,
    workflowConcurrency,
    workflowSettingsOpen,
    workflowTriggers,
    triggerEditorOpen,
    triggerEditorCreating,
    triggerEditorError,
    triggerDraft,
    triggerJson,
    workflowEditorMode,
    workflowLayoutDirection,
    workflowInspectorMode,
    stepEditorOpen,
    stepEditorCreating,
    stepEditorError,
    workflowRuns,
    workflowLayoutVersion,
    selectedWorkflowRunId,
    workflowRunDetail,
    workflowRunGates,
    workflowNodeDetailExtra,
    selectedStepId,
    inlineEditNodeId,
    selectedWorkflowRunNodeId,
    selectedWorkflowNodeRunId,
    stepEditor,
    selectedWorkflow,
    canRunWorkflow,
    selectedWorkflowInputType,
    selectedWorkflowHasInputs,
    runInputOpen,
    runInputDraft,
    runInputDebug,
    closeRunInput,
    confirmRunInput,
    canManageWorkflowTriggers,
    canRemoveSelectedStep,
    filteredWorkflows,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    graphValidationIssues,
    workflowRunWorkflow,
    runGraphNodes,
    runGraphEdges,
    selectedNode,
    selectedStepKindLocked,
    selectedGraphEdgeId,
    selectedGraphEdge,
    selectedNodeIssues,
    selectedEdgeIssues,
    selectedNodePendingApproval,
    canStepWorkflowRun,
    canContinueWorkflowRun,
    canPauseWorkflowRun,
    canResumeWorkflowRun,
    canCancelWorkflowRun,
    debugState,
    controlState,
    isDebugRun,
    currentBreakpoints,
    isBreakpointed,
    workflowNodeKinds,
    directTransitionKeys,
    refreshWorkflows,
    clearServiceState,
    selectWorkflow,
    addWorkflow,
    saveSelectedWorkflow: saveSelectedWorkflowBundle,
    deleteSelectedWorkflow,
    duplicateSelectedWorkflow,
    runSelectedWorkflow,
    runSelectedWorkflowDebug,
    stepSelectedWorkflowRun,
    continueSelectedWorkflowRun,
    pauseSelectedWorkflowRun,
    resumeSelectedWorkflowRun,
    cancelSelectedWorkflowRun,
    patchSelectedWorkflowRunDebug,
    toggleBreakpoint,
    runToCursor,
    skipCurrentNode,
    rerunCurrentNode,
    replaySelectedWorkflowRun,
    renameSelectedWorkflowRun,
    openRunIds,
    openRunInTab,
    closeRunTab,
    activateRunTab,
    runDetailById,
    watchExpressionsForActiveWorkflow,
    addWatchExpression,
    removeWatchExpression,
    fetchWorkflowRunsForSelected,
    fetchRecentWorkflowRuns,
    selectWorkflowRun,
    fetchWorkflowRunDetail,
    refreshWorkflowRunGates,
    resolveWorkflowRunGate,
    setWorkflowRunDetail,
    selectWorkflowRunNode,
    addWorkflowStep,
    addWorkflowNode,
    addConnectedWorkflowNode,
    applyInlineNodeEdit,
    clearWorkflowGraphSelection,
    submitInlineNodeEdit,
    removeWorkflowStep,
    removeWorkflowNode,
    removeWorkflowEdgeById,
    openEdgeEditorDraft,
    selectGraphEdge,
    applyEdgeEditorDraft,
    moveEdgeEditorItem,
    moveSelectedEdge,
    reverseSelectedEdgeHandles,
    setEdgeLabelOffset,
    setEdgeLabelAnchor,
    workflowEdgeOptions,
    applyGraphEdgeSemantic,
    applyStepEditor,
    populateStepEditor,
    updateSelectedWorkflowNodeDetail,
    onGraphNodeClick,
    onGraphNodeDoubleClick,
    onGraphNodeDragStop,
    onGraphNodesChange,
    onGraphConnect,
    onGraphEdgeClick,
    onGraphEdgeUpdate,
    onGraphEdgesChange,
    autoArrangeWorkflowNodes,
    isDirty,
    syncWorkflowJson,
    syncWorkflowDraftToJson,
    syncWorkflowWdl,
    exportWorkflowWdl,
    exportWorkflowPack,
    ensureWorkflowNodes,
    addConditionBranchEditor,
    removeConditionBranchEditor,
    addAssertionEditor,
    removeAssertionEditor,
    addSwitchCaseEditor,
    removeSwitchCaseEditor,
    addPercentageBucketEditor,
    removePercentageBucketEditor,
    addNodeRefEditor,
    removeNodeRefEditor,
    openStepEditor,
    closeStepEditor,
    submitStepEditor,
    duplicateSelectedStep,
    moveWorkflowSelection,
    openWorkflowSettings,
    closeWorkflowSettings,
    refreshWorkflowTriggers,
    addWorkflowTrigger,
    editWorkflowTrigger,
    closeTriggerEditor,
    setTriggerKind,
    submitWorkflowTrigger,
    deleteSelectedWorkflowTrigger,
    triggerCronSummary,
    triggerDateForInput,
    markWorkflowDirty,
  };
});
