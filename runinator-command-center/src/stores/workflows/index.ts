import { defineStore } from "pinia";
import { computed, reactive, ref, watch } from "vue";
import {
  cancelWorkflowRun,
  compileWdl,
  continueWorkflowRun,
  createWorkflowRun,
  decompileToWdl,
  deleteWorkflow,
  deleteWorkflowTrigger,
  duplicateWorkflow,
  downloadBlob,
  downloadTextFile,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  fetchWorkflowTriggers,
  fetchWorkflows,
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
  type WorkflowWdlSaveRequest
} from "../../api/commandCenterApi";
import type { Edge } from "@vue-flow/core";
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, RuninatorType, WorkflowDefinition, WorkflowEdgeEditorDraft, WorkflowLayoutDirection, WorkflowNodeKind, WorkflowRunDetail, WorkflowTrigger, WorkflowTriggerKind, WorkflowValidationIssue } from "../../types/models";
import { pretty } from "../../utils/format";
import { cloneJson, parseObject, parseRequiredJson, parseRequiredObject } from "../../utils/json";
import { isBlankValue } from "../../utils/values";
import { createZip, type ZipEntry } from "../../utils/zip";
import {
  applyWorkflowEdgeEditorDraft,
  applyWorkflowInlineNodeEdit,
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
  workflowRunSearchText
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
  type BranchPolicyName,
  type SwitchCaseEditor
} from "./helpers";
import { useAppStore } from "../app";
import { useProvidersStore } from "../providers";
export { buildInputSkeleton, newWorkflowDraft, newWorkflowTriggerDraft } from "./helpers";
import { useResourcesStore } from "../resources";

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<string | null>(null);
  const workflowDraft = reactive<WorkflowDefinition>(newWorkflowDraft());
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
  const workflowRuns = ref<RunSummary[]>([]);
  const workflowLayoutVersion = ref(0);
  const workflowRunsByRunId = computed(() => {
    const groups: Record<string, RunSummary[]> = {};
    for (const run of workflowRuns.value) {
      const runId = run.id;
      if (!groups[runId]) groups[runId] = [];
      groups[runId].push(run);
    }
    return groups;
  });
  const recentWorkflowRuns = computed(() => {
    const query = app.normalizedSearch;
    let list = workflowRuns.value;
    if (query) {
      list = list.filter(r => workflowRunSearchText(r, workflowNameForRun(r)).includes(query));
    }
    return list.slice(0, 50);
  });
  const selectedWorkflowRunId = ref<string | null>(null);
  const workflowRunDetail = ref<WorkflowRunDetail | null>(null);
  const openRunIds = ref<string[]>([]);
  const runDetailById = reactive(new Map<string, WorkflowRunDetail | null>());
  const MAX_OPEN_RUN_TABS = 8;
  const latestWorkflowRunPushVersion = new Map<string, number>();
  const latestWorkflowRunHttpRequest = new Map<string, number>();
  let nextWorkflowRunDetailVersion = 0;
  let nextWorkflowRunHttpRequestId = 0;
  let nextBreakpointMutationId = 0;
  let pendingBreakpointPatch: { runId: string; breakpoints: string[]; mutationId: number } | null = null;
  const workflowNodeDetailExtra = ref("");
  const selectedStepId = ref("");
  const selectedGraphEdgeId = ref("");
  const selectedWorkflowRunNodeId = ref("");
  const selectedWorkflowNodeRunId = ref<string | null>(null);
  const stepEditor = reactive({
    id: "",
    name: "",
    kind: "action" as WorkflowNodeKind,
    approval_type: "generic",
    approval_prompt: "Approval required",
    gate_kind: "manual",
    gate_when_json: "{}",
    gate_poll_interval: 30,
    gate_timeout: 0,
    gate_label: "",
    signal_name: "signal",
    condition_fallback: "",
    condition_branches: [] as Array<{ when_json: string; target: string }>,
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
    parallel_branches: [] as string[],
    join_wait_for: [] as string[],
    join_mode: "all" as BranchPolicyName,
    try_body: "",
    try_catch: "",
    try_finally: "",
    map_items_json: "[]",
    map_target: "",
    map_concurrency: 1,
    race_branches: [] as string[],
    race_winner: "first_success" as BranchPolicyName,
    output_event_type: "workflow.output",
    output_data_json: "{}",
    input_prompt: "Provide input",
    config_name_json: "\"\"",
    config_metadata_json: "{}",
    subflow_id: "",
    subflow_parameters_json: "{}",
    locked: false,
    skipped: false,
    max_attempts: 1,
    timeout_seconds: 0,
    action_name: "",
    action_function: "",
    parameters_json: "{}",
    transitions_json: "{}"
  });

  const isDirty = ref(false);
  let workflowJsonSyncTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowWdlSyncTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowWdlRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowJsonWriteReleaseTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowWdlWriteReleaseTimer: ReturnType<typeof setTimeout> | null = null;
  let stepEditorApplyTimer: ReturnType<typeof setTimeout> | null = null;
  let workflowJsonWriteGuard = false;
  let workflowWdlWriteGuard = false;
  let stepEditorHydrating = false;
  let stepEditorBaselineDefinition: JsonRecord | null = null;

  const app = useAppStore();
  watch(workflowJson, () => {
    if (workflowJsonWriteGuard || workflowEditorMode.value !== "json") return;
    scheduleWorkflowJsonSync();
  });
  watch(workflowWdl, () => {
    // the graph and wdl panes are live side by side; a user wdl edit always compiles into the draft.
    // a silent programmatic write (guard) or a non-representable draft (error) must not.
    if (workflowWdlWriteGuard || workflowWdlError.value) return;
    workflowEditorMode.value = "wdl";
    scheduleWorkflowWdlSync();
  });
  watch(
    stepEditor,
    () => {
      if (stepEditorHydrating || !stepEditorOpen.value) return;
      scheduleStepEditorApply();
    },
    { deep: true }
  );
  const selectedWorkflow = computed(() => workflows.value.find((workflow) => workflow.id === selectedWorkflowId.value) ?? null);
  const canRunWorkflow = computed(() => Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id));
  const selectedWorkflowInputType = computed<RuninatorType | null>(() => selectedWorkflow.value?.input_type ?? null);
  const selectedWorkflowHasInputs = computed(() => {
    const ty = selectedWorkflowInputType.value;
    return Boolean(ty && ty.type === "struct" && Object.keys(ty.fields).length > 0);
  });
  const canManageWorkflowTriggers = computed(() => Boolean(workflowDraft.id));
  const canStepWorkflowRun = computed(() => workflowRunDetail.value?.run.status === "debug_paused");
  const debugState = computed<Record<string, any> | null>(() => {
    const debug = workflowRunDetail.value?.run.state?.debug;
    if (debug && typeof debug === "object" && !Array.isArray(debug)) return debug as Record<string, any>;
    return null;
  });
  const isDebugRun = computed(() => Boolean(debugState.value?.enabled));
  const canContinueWorkflowRun = computed(() => workflowRunDetail.value?.run.status === "debug_paused");
  const controlState = computed<Record<string, any> | null>(() => {
    const control = workflowRunDetail.value?.run.state?.control;
    if (control && typeof control === "object" && !Array.isArray(control)) return control as Record<string, any>;
    return null;
  });
  const pauseRequested = computed(() => Boolean(controlState.value?.pause_requested));
  const canPauseWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;
    return Boolean(status && ["running", "waiting", "approval_required"].includes(status) && !pauseRequested.value);
  });
  const canResumeWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;
    return status === "paused" || (status === "debug_paused" && pauseRequested.value);
  });
  const canCancelWorkflowRun = computed(() => {
    const status = workflowRunDetail.value?.run.status;
    if (!status) return false;
    return !["succeeded", "failed", "canceled", "timed_out"].includes(status);
  });
  const currentBreakpoints = computed<string[]>(() => {
    const list = debugState.value?.breakpoints;
    return Array.isArray(list) ? list.filter((id): id is string => typeof id === "string") : [];
  });
  function isBreakpointed(nodeId: string): boolean {
    return currentBreakpoints.value.includes(nodeId);
  }
  const selectedStepKindLocked = computed(() => {
    const node = workflowDraft.definition?.nodes?.find((item: JsonRecord) => item.id === selectedStepId.value);
    return isLockedWorkflowNode(node);
  });
  const canRemoveSelectedStep = computed(() => {
    const node = workflowDraft.definition?.nodes?.find((item: JsonRecord) => item.id === selectedStepId.value);
    return Boolean(node && !isLockedWorkflowNode(node));
  });
  const filteredWorkflows = computed(() => {
    const query = app.normalizedSearch;
    if (!query) return workflows.value;
    return workflows.value.filter((workflow) => [workflow.name, String(workflow.id ?? ""), String(workflow.version)].some((value) => value.toLowerCase().includes(query)));
  });
  const workflowRunDetailText = computed(() => {
    const detail = workflowRunDetail.value;
    if (!detail) return "";
    const lines = [
      `Run ${detail.run.id}: ${detail.run.status}`,
      `Started: ${formatMaybeDate(detail.run.started_at)}`,
      `Finished: ${formatMaybeDate(detail.run.finished_at)}`
    ];
    if (detail.run.message) lines.push(`Message: ${detail.run.message}`);
    for (const step of detail.nodes) {
      lines.push(`${step.node_id}: ${step.status}, attempt ${step.attempt}, node run ${step.id}${step.message ? `, ${step.message}` : ""}`);
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
    () => new Map(workflows.value.filter((w) => w.id != null).map((w) => [w.id as string, w.name]))
  );
  const graphNodes = computed(() => buildGraphNodes(workflowDraft, null, subflowNames.value, useProvidersStore().providers));
  const graphEdges = computed(() => buildGraphEdges(workflowDraft));
  const graphValidationIssues = computed(() => validateWorkflowIssues(workflowDraft.definition, useProvidersStore().providers));
  const workflowRunWorkflow = computed(() => {
    const snapshot = workflowRunDetail.value?.run.workflow_snapshot;
    if (snapshot) return snapshot;
    const workflowId = workflowRunDetail.value?.run.workflow_id ?? workflowRuns.value.find((run) => run.id === selectedWorkflowRunId.value)?.workflow_id;
    return workflows.value.find((workflow) => workflow.id === workflowId) ?? null;
  });
  const runGraphNodes = computed(() => workflowRunWorkflow.value
    ? buildGraphNodes(workflowRunWorkflow.value, workflowRunDetail.value, subflowNames.value, useProvidersStore().providers).map((node) => ({
        ...node,
        data: { ...(node.data as JsonRecord), readOnly: true }
      }))
    : []);
  const runGraphEdges = computed(() => workflowRunWorkflow.value ? buildGraphEdges(workflowRunWorkflow.value) : []);
  const selectedNode = computed(() => ensureWorkflowNodes().find((item: JsonRecord) => item.id === selectedStepId.value) ?? null);
  const selectedGraphEdge = computed(() => graphEdges.value.find((edge: Edge) => edge.id === selectedGraphEdgeId.value) ?? null);
  const selectedNodeIssues = computed<WorkflowValidationIssue[]>(() => graphValidationIssues.value.filter((issue) => issue.nodeId === selectedStepId.value));
  const selectedEdgeIssues = computed<WorkflowValidationIssue[]>(() => {
    const edge = selectedGraphEdge.value;
    if (!edge) return [];
    const data = edge.data as any;
    const semanticKey = data?.transitionKey ?? (typeof data?.branchIndex === "number" ? `branches.${data.branchIndex}` : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    return graphValidationIssues.value.filter((issue) => issue.edgeKey === `${edge.source}:${semanticKey}`);
  });
  const selectedNodePendingApproval = computed(() => {
    const detail = workflowRunDetail.value;
    if (!detail || !selectedStepId.value) return null;
    return detail.nodes.filter((node) => node.node_id === selectedStepId.value && ["waiting", "approval_required", "pending"].includes(String(node.status))).at(-1) ?? null;
  });

  async function refreshWorkflows() {
    console.info("[command-center] refreshing workflows");
    workflows.value = await app.runOperation("Refreshing workflows", () => fetchWorkflows()).catch(() => []);
    if (!selectedWorkflowId.value && workflows.value.length > 0) selectedWorkflowId.value = workflows.value[0].id;
    const workflow = workflows.value.find((item) => item.id === selectedWorkflowId.value) ?? workflows.value[0];
    if (workflow && !isDirty.value) await selectWorkflow(workflow);
  }

  function clearServiceState() {
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
    clearWorkflowTriggerState();
    if (isDirty.value) return;
    selectedWorkflowId.value = null;
    Object.assign(workflowDraft, newWorkflowDraft());
    setWorkflowJsonSilently(pretty(workflowDraft.definition ?? { nodes: [] }));
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
      delete transitions[key];
    }
    stepEditor.transitions_json = pretty(transitions);
    isDirty.value = true;
  }

  function selectWorkflow(workflow: WorkflowDefinition) {
    const isSwitch = selectedWorkflowId.value !== workflow.id;
    selectedWorkflowId.value = workflow.id;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflow)));
    workflowConcurrency.value = Number(workflowDraft.definition?.concurrency ?? 1);
    setWorkflowJsonSilently(pretty(workflowDraft.definition ?? { nodes: [] }));
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
    selectWorkflow(workflow);
  }

  // open the input form when the workflow declares inputs, otherwise launch immediately.
  async function runSelectedWorkflow(debug = false) {
    const workflow = selectedWorkflow.value;
    if (!workflow?.id || !workflow.enabled) return app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
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
    if (!workflow?.id || !workflow.enabled) return app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
    const response = await app.runOperation(
      debug ? `Running workflow ${workflow.name} in debug mode` : `Running workflow ${workflow.name}`,
      () => createWorkflowRun(workflow.id!, { debug, parameters })
    );
    selectedWorkflowRunId.value = response.id;
    app.setStatus(`${debug ? "Debug workflow run" : "Workflow run"} queued: ${response.id}`);
    await fetchWorkflowRunDetail(response.id);
    await fetchRecentWorkflowRuns();
    app.activeTab = "Runs";
  }

  async function stepSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Stepping workflow run ${runId}`, () => stepWorkflowRun(runId));
    if (response.success === false) {
      app.setError(response.message || "Failed to step workflow run");
      return;
    }
    app.setStatus(response.message || `Workflow run ${runId} stepped`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function continueSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canContinueWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Continuing workflow run ${runId}`, () => continueWorkflowRun(runId));
    if (response.success === false) {
      app.setError(response.message || "Failed to continue workflow run");
      return;
    }
    app.setStatus(response.message || `Workflow run ${runId} continued`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function cancelSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canCancelWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Canceling workflow run ${runId}`, () => cancelWorkflowRun(runId));
    if (response.success === false) {
      app.setError(response.message || "Failed to cancel workflow run");
      return;
    }
    app.setStatus(response.message || `Workflow run ${runId} canceled`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function pauseSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canPauseWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Pausing workflow run ${runId}`, () => pauseWorkflowRun(runId));
    if (response.success === false) {
      app.setError(response.message || "Failed to pause workflow run");
      return;
    }
    app.setStatus(response.message || `Workflow run ${runId} pause requested`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function resumeSelectedWorkflowRun() {
    if (!workflowRunDetail.value || !canResumeWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Resuming workflow run ${runId}`, () => resumeWorkflowRun(runId));
    if (response.success === false) {
      app.setError(response.message || "Failed to resume workflow run");
      return;
    }
    app.setStatus(response.message || `Workflow run ${runId} resumed`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function patchSelectedWorkflowRunDebug(patch: WorkflowDebugPatch) {
    if (!workflowRunDetail.value || !isDebugRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Updating debug settings for run ${runId}`, () => patchWorkflowRunDebug(runId, patch));
    if (response.success === false) {
      app.setError(response.message || "Failed to update debug settings");
      return;
    }
    await fetchWorkflowRunDetail(runId, true);
  }

  async function toggleBreakpoint(nodeId: string) {
    if (!workflowRunDetail.value || !isDebugRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const current = currentBreakpoints.value;
    const next = current.includes(nodeId)
      ? current.filter((id) => id !== nodeId)
      : [...current, nodeId];
    const mutationId = ++nextBreakpointMutationId;
    pendingBreakpointPatch = { runId, breakpoints: next, mutationId };
    applyBreakpointPatch(workflowRunDetail.value, next);
    try {
      const response = await app.runOperation(`Updating debug settings for run ${runId}`, () => patchWorkflowRunDebug(runId, { breakpoints: next }));
      if (response.success === false) {
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
    if (!workflowRunDetail.value || !isDebugRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Running to cursor ${nodeId}`, () => runToCursorWorkflowRun(runId, nodeId));
    if (response.success === false) {
      app.setError(response.message || "Failed to run to cursor");
      return;
    }
    app.setStatus(response.message || `Running to ${nodeId}`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function skipCurrentNode(outputJson: any, message?: string) {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Skipping current node`, () => skipWorkflowNode(runId, outputJson, message));
    if (response.success === false) {
      app.setError(response.message || "Failed to skip node");
      return;
    }
    app.setStatus(response.message || `Node skipped`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function rerunCurrentNode(parameters: any) {
    if (!workflowRunDetail.value || !canStepWorkflowRun.value) return;
    const runId = workflowRunDetail.value.run.id;
    const response = await app.runOperation(`Re-running current node`, () => rerunWorkflowNode(runId, parameters));
    if (response.success === false) {
      app.setError(response.message || "Failed to re-run node");
      return;
    }
    app.setStatus(response.message || `Node re-running`);
    await fetchWorkflowRunDetail(runId, true);
  }

  async function replaySelectedWorkflowRun(runId?: string, fromStepId?: string) {
    const targetId = runId ?? workflowRunDetail.value?.run.id;
    if (!targetId) return;
    const label = fromStepId
      ? `Replaying workflow run ${targetId} from step ${fromStepId}`
      : `Replaying workflow run ${targetId}`;
    const created = await app
      .runOperation(label, () => replayWorkflowRunApi(targetId, { fromStepId }))
      .catch((error) => {
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
    if (!runId) return;
    const response = await app
      .runOperation(`Renaming run ${runId}`, () => renameWorkflowRunApi(runId, name))
      .catch((error) => {
        app.setError(String(error));
        return null;
      });
    if (!response) return;
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
    if (!storage) return {};
    const result: Record<string, string[]> = {};
    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);
      if (!key || !key.startsWith(WATCH_STORAGE_PREFIX)) continue;
      const id = key.slice(WATCH_STORAGE_PREFIX.length);
      if (!id) continue;
      try {
        const parsed = JSON.parse(storage.getItem(key) ?? "[]");
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
    if (!workflowId) return [];
    return watchExpressionsByWorkflowId.value[workflowId] ?? [];
  });

  function persistWatchExpressions(workflowId: string, list: string[]) {
    const storage = typeof window !== "undefined" ? window.localStorage : undefined;
    if (!storage) return;
    storage.setItem(`${WATCH_STORAGE_PREFIX}${workflowId}`, JSON.stringify(list));
  }

  function addWatchExpression(expression: string) {
    const workflowId = workflowRunWorkflow.value?.id;
    if (!workflowId || !expression.trim()) return;
    const existing = watchExpressionsByWorkflowId.value[workflowId] ?? [];
    if (existing.includes(expression)) return;
    const next = [...existing, expression];
    watchExpressionsByWorkflowId.value = { ...watchExpressionsByWorkflowId.value, [workflowId]: next };
    persistWatchExpressions(workflowId, next);
  }

  function removeWatchExpression(expression: string) {
    const workflowId = workflowRunWorkflow.value?.id;
    if (!workflowId) return;
    const existing = watchExpressionsByWorkflowId.value[workflowId] ?? [];
    const next = existing.filter((e) => e !== expression);
    watchExpressionsByWorkflowId.value = { ...watchExpressionsByWorkflowId.value, [workflowId]: next };
    persistWatchExpressions(workflowId, next);
  }

  async function fetchWorkflowRunsForSelected(workflowId: string) {
    console.info("[command-center] refreshing workflow runs", { workflowId });
    workflowRuns.value = await app.runOperation("Loading workflow runs", () => fetchWorkflowRuns(workflowId)).catch(() => []);
    if (!workflowRuns.value.some((run) => run.id === selectedWorkflowRunId.value)) {
      selectedWorkflowRunId.value = workflowRuns.value[0]?.id ?? null;
    }
  }

  async function fetchRecentWorkflowRuns() {
    console.info("[command-center] refreshing recent workflow runs");
    workflowRuns.value = await app.runOperation("Loading workflow runs", () => fetchWorkflowRuns()).catch(() => []);
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
    if (!runId) return;
    const ids = openRunIds.value;
    if (!ids.includes(runId)) {
      // Cap the tab count by evicting the oldest non-active tab.
      if (ids.length >= MAX_OPEN_RUN_TABS) {
        const victim = ids.find((id) => id !== selectedWorkflowRunId.value);
        if (victim) closeRunTab(victim);
      }
      openRunIds.value = [...ids, runId];
    }
    if (!runDetailById.has(runId)) {
      runDetailById.set(runId, null);
    }
  }

  function activateRunTab(runId: string) {
    if (!runId) return;
    if (!openRunIds.value.includes(runId)) openRunInTab(runId);
    selectedWorkflowRunId.value = runId;
    workflowRunDetail.value = runDetailById.get(runId) ?? null;
    workflowNodeDetailExtra.value = "";
    selectedWorkflowRunNodeId.value = workflowRunDetail.value?.nodes[0]?.node_id ?? "";
    if (!runDetailById.get(runId)) {
      void fetchWorkflowRunDetail(runId, true);
    }
  }

  function closeRunTab(runId: string) {
    const ids = openRunIds.value;
    const index = ids.indexOf(runId);
    if (index === -1) return;
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
      : await app.runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId)).catch(() => null);
    applyWorkflowRunDetail(detail, { source: "http", requestStartedVersion, requestId });
  }

  function setWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    if (detail) latestWorkflowRunPushVersion.set(detail.run.id, ++nextWorkflowRunDetailVersion);
    applyWorkflowRunDetail(detail, { source: "ws" });
  }

  function selectWorkflowRunNode(nodeId: string) {
    selectedWorkflowRunNodeId.value = nodeId;
    updateSelectedWorkflowNodeDetail();
  }

  function applyWorkflowRunDetail(detail: WorkflowRunDetail | null, metadata: { source: "http"; requestStartedVersion: number; requestId: number } | { source: "ws" } = { source: "ws" }) {
    if (detail && metadata.source === "http") {
      const latestPushVersion = latestWorkflowRunPushVersion.get(detail.run.id) ?? 0;
      const latestRequestId = latestWorkflowRunHttpRequest.get(detail.run.id) ?? 0;
      if (latestPushVersion > metadata.requestStartedVersion || latestRequestId !== metadata.requestId) {
        console.info("[command-center] dropped stale workflow run detail", { runId: detail.run.id });
        return;
      }
    }
    if (detail) confirmPendingBreakpointPatch(detail);
    if (detail) {
      runDetailById.set(detail.run.id, detail);
      if (!openRunIds.value.includes(detail.run.id)) {
        openRunIds.value = [...openRunIds.value, detail.run.id].slice(-MAX_OPEN_RUN_TABS);
      }
      if (selectedWorkflowRunId.value === null) {
        selectedWorkflowRunId.value = detail.run.id;
      }
    }
    const isActiveRun = detail
      ? detail.run.id === selectedWorkflowRunId.value
      : true;
    if (isActiveRun) {
      workflowRunDetail.value = detail;
      reapplyPendingBreakpointPatch();
      workflowNodeDetailExtra.value = "";
      if (!detail?.nodes.some((node) => node.node_id === selectedWorkflowRunNodeId.value)) {
        selectedWorkflowRunNodeId.value = detail?.nodes[0]?.node_id ?? "";
      }
    }
    if (detail) {
      const resources = useResourcesStore();
      const hasWaiting = detail.nodes.some(n =>
        n.status === "waiting" || n.status === "approval_required" || n.status === "pending"
      );
      if (hasWaiting) resources.refreshResources();
    }
  }

  function reapplyPendingBreakpointPatch() {
    if (!workflowRunDetail.value || !pendingBreakpointPatch) return;
    if (workflowRunDetail.value.run.id !== pendingBreakpointPatch.runId) return;
    applyBreakpointPatch(workflowRunDetail.value, pendingBreakpointPatch.breakpoints);
  }

  function confirmPendingBreakpointPatch(detail: WorkflowRunDetail) {
    if (!pendingBreakpointPatch || detail.run.id !== pendingBreakpointPatch.runId) return;
    if (sameBreakpoints(readBreakpoints(detail), pendingBreakpointPatch.breakpoints)) {
      pendingBreakpointPatch = null;
    }
  }

  function clearPendingBreakpointPatch(runId: string, mutationId: number) {
    if (pendingBreakpointPatch?.runId === runId && pendingBreakpointPatch.mutationId === mutationId) {
      pendingBreakpointPatch = null;
      return true;
    }
    return false;
  }

  function applyBreakpointPatch(detail: WorkflowRunDetail | null, breakpoints: string[]) {
    const debug = (detail?.run.state as any)?.debug;
    if (debug && typeof debug === "object" && !Array.isArray(debug)) {
      debug.breakpoints = [...breakpoints];
    }
  }

  function readBreakpoints(detail: WorkflowRunDetail): string[] {
    const debug = (detail.run.state as any)?.debug;
    const breakpoints = debug && typeof debug === "object" && !Array.isArray(debug) ? debug.breakpoints : null;
    return Array.isArray(breakpoints) ? breakpoints.filter((id): id is string => typeof id === "string") : [];
  }

  function sameBreakpoints(left: string[], right: string[]) {
    const normalizedLeft = [...new Set(left)].sort();
    const normalizedRight = [...new Set(right)].sort();
    if (normalizedLeft.length !== normalizedRight.length) return false;
    return normalizedLeft.every((id, index) => id === normalizedRight[index]);
  }

  async function addWorkflowStep() {
    await addWorkflowNode("action");
  }

  async function addWorkflowNode(kind: WorkflowNodeKind) {
    const nodes = ensureWorkflowNodes();
    const newNode = createWorkflowNode(kind, nodes);
    stripNewNodeConnections(newNode);
    const position = graphCentroidPosition();
    const endIndex = nodes.findIndex((node: JsonRecord) => node.kind === "end");
    if (endIndex >= 0) nodes.splice(endIndex, 0, newNode);
    else nodes.push(newNode);
    setGraphNodePosition(newNode.id, position);
    syncWorkflowDraftToJson();
    populateStepEditor(newNode.id);
    openStepEditor(newNode.id, true);
  }

  async function addConnectedWorkflowNode(kind: WorkflowNodeKind = "action") {
    return addWorkflowNode(kind);
  }

  function removeWorkflowStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) return;
    removeWorkflowNode(selectedStepId.value);
  }

  function removeWorkflowNode(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);
    if (!node || isLockedWorkflowNode(node)) return;
    workflowDraft.definition.nodes = ensureWorkflowNodes().filter((item: JsonRecord) => item.id !== nodeId);
    removeWorkflowNodeReferences(workflowDraft.definition, nodeId);
    delete workflowDraft.definition.ui?.layout?.nodes?.[nodeId];
    if (selectedStepId.value === nodeId) selectedStepId.value = "";
    syncWorkflowDraftToJson();
  }

  function applyInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    const previousId = nodeId;
    const result = applyWorkflowInlineNodeEdit(workflowDraft.definition, nodeId, nextId, inlineValue);
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
    selectedGraphEdgeId.value = "";
  }

  function submitInlineNodeEdit(nodeId: string, nextId: string, inlineValue: string): boolean {
    if (!applyInlineNodeEdit(nodeId, nextId, inlineValue)) return false;
    clearWorkflowGraphSelection();
    return true;
  }

  function applyStepEditor(): boolean {
    if (stepEditorApplyTimer) {
      clearTimeout(stepEditorApplyTimer);
      stepEditorApplyTimer = null;
    }
    stepEditorError.value = "";
    if (!selectedStepId.value) return false;
    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === selectedStepId.value);
    if (index < 0) return false;
    if (isLockedWorkflowNode(nodes[index]) && stepEditor.kind !== nodes[index].kind) {
      const message = `${nodes[index].kind} node kind cannot be changed`;
      stepEditorError.value = message;
      app.setError(message);
      return false;
    }
    const parameters = parseRequiredObject(stepEditor.parameters_json);
    const transitions = parseRequiredObject(stepEditor.transitions_json);
    if (!parameters || !transitions) {
      const message = parameters ? "Node transitions must be a JSON object" : "Step parameters must be a JSON object";
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
    const next = { ...nodes[index] };
    next.id = stepEditor.id.trim();
    if (!next.id) {
      stepEditorError.value = "Step ID is required";
      return false;
    }
    const trimmedName = stepEditor.name.trim();
    if (trimmedName) next.name = trimmedName;
    else delete next.name;
    next.kind = stepEditor.kind;
    if (next.kind === "action") {
      next.action = {
        ...(typeof next.action === "object" && next.action ? next.action : {}),
        provider: stepEditor.action_name,
        function: stepEditor.action_function,
        timeout_seconds: stepEditor.timeout_seconds > 0 ? stepEditor.timeout_seconds : next.action?.timeout_seconds ?? 300,
        configuration: parameters
      };
    } else {
      delete next.action;
    }
    next.retry = { max_attempts: stepEditor.max_attempts };
    if (stepEditor.timeout_seconds > 0) next.timeout_seconds = stepEditor.timeout_seconds;
    else delete next.timeout_seconds;
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
      next.parameters = { ...parameters, approval_type: stepEditor.approval_type || "generic", prompt: stepEditor.approval_prompt || "Approval required" };
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
      const pollInterval = Number(stepEditor.gate_poll_interval ?? 0);
      if (pollInterval > 0) next.parameters.poll_interval = pollInterval;
      else delete next.parameters.poll_interval;
      const timeout = Number(stepEditor.gate_timeout ?? 0);
      if (timeout > 0) next.parameters.timeout = timeout;
      else delete next.parameters.timeout;
      if (stepEditor.gate_label.trim()) next.parameters.label = stepEditor.gate_label.trim();
      else delete next.parameters.label;
    }
    if (next.kind === "signal") {
      next.parameters = { ...parameters, name: stepEditor.signal_name.trim() || "signal" };
    }
    if (next.kind === "condition") {
      next.transitions = { ...transitions, branches: [] };
      for (const [branchIndex, branch] of stepEditor.condition_branches.entries()) {
        const when = parseRequiredObject(branch.when_json);
        if (!when) {
          stepEditorError.value = `Condition branch ${branchIndex + 1} must be a JSON object`;
          app.setError(stepEditorError.value);
          return false;
        }
        if (!branch.target) {
          stepEditorError.value = `Condition branch ${branchIndex + 1} needs a target`;
          app.setError(stepEditorError.value);
          return false;
        }
        setConditionBranch(next, branchIndex, when, branch.target);
      }
      if (stepEditor.condition_fallback) next.transitions.next = nodeRef(stepEditor.condition_fallback);
      else delete next.transitions.next;
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
        seconds: Math.max(0, Number(stepEditor.wait_seconds ?? 0))
      };
      if (stepEditor.wait_initial_status.trim()) next.wait.initial_status = stepEditor.wait_initial_status.trim();
      else delete next.wait.initial_status;
      if (stepEditor.wait_until_status.trim()) next.wait.until_status = stepEditor.wait_until_status.trim();
      else delete next.wait.until_status;
    } else {
      delete next.wait;
    }
    if (next.kind === "loop") {
      const items = parseStepJson("Loop items", stepEditor.loop_items_json);
      if (!items.ok) return false;
      next.parameters = { ...parameters, items: items.value };
      if (stepEditor.loop_target) next.parameters.target = nodeRef(stepEditor.loop_target);
      else delete next.parameters.target;
      next.max_iterations = Math.max(1, Number(stepEditor.loop_max_iterations ?? 1));
    } else {
      delete next.max_iterations;
    }
    if (next.kind === "switch") {
      const value = parseStepJson("Switch value", stepEditor.switch_value_json);
      if (!value.ok) return false;
      const cases: JsonRecord[] = [];
      for (const [caseIndex, switchCase] of stepEditor.switch_cases.entries()) {
        if (!switchCase.target) {
          setStepEditorError(`Switch case ${caseIndex + 1} needs a target`);
          return false;
        }
        const match = parseStepJson(`Switch case ${caseIndex + 1}`, switchCase.match_json);
        if (!match.ok) return false;
        const serialized: JsonRecord = { target: nodeRef(switchCase.target) };
        if (switchCase.match_kind === "when") serialized.when = match.value;
        else if (switchCase.match_kind === "exists") serialized.exists = Boolean(match.value);
        else serialized[switchCase.match_kind] = match.value;
        cases.push(serialized);
      }
      next.parameters = { ...parameters, value: value.value, cases };
      if (stepEditor.switch_default) next.parameters.default = nodeRef(stepEditor.switch_default);
      else delete next.parameters.default;
    }
    if (next.kind === "parallel") {
      next.parameters = {
        ...parameters,
        branches: stepEditor.parallel_branches.filter(Boolean).map(nodeRef)
      };
    }
    if (next.kind === "join") {
      next.parameters = {
        ...parameters,
        wait_for: stepEditor.join_wait_for.filter(Boolean).map(nodeRef),
        mode: stepEditor.join_mode
      };
    }
    if (next.kind === "try") {
      next.parameters = { ...parameters };
      if (stepEditor.try_body) next.parameters.body = nodeRef(stepEditor.try_body);
      else delete next.parameters.body;
      if (stepEditor.try_catch) next.parameters.catch = nodeRef(stepEditor.try_catch);
      else delete next.parameters.catch;
      if (stepEditor.try_finally) next.parameters.finally = nodeRef(stepEditor.try_finally);
      else delete next.parameters.finally;
    }
    if (next.kind === "map") {
      const items = parseStepJson("Map items", stepEditor.map_items_json);
      if (!items.ok) return false;
      next.parameters = {
        ...parameters,
        items: items.value,
        concurrency: Math.max(1, Number(stepEditor.map_concurrency ?? 1))
      };
      if (stepEditor.map_target) next.parameters.target = nodeRef(stepEditor.map_target);
      else delete next.parameters.target;
    }
    if (next.kind === "race") {
      next.parameters = {
        ...parameters,
        branches: stepEditor.race_branches.filter(Boolean).map(nodeRef),
        winner: stepEditor.race_winner
      };
    }
    if (next.kind === "output") {
      const data = parseStepJson("Output data", stepEditor.output_data_json);
      if (!data.ok) return false;
      next.parameters = {
        ...parameters,
        event_type: stepEditor.output_event_type.trim() || "workflow.output",
        data: data.value
      };
    }
    if (next.kind === "input") {
      next.parameters = {
        ...parameters,
        prompt: stepEditor.input_prompt.trim() || "Provide input"
      };
    }
    if (next.kind === "config") {
      const name = parseStepJson("Config name", stepEditor.config_name_json);
      if (!name.ok) return false;
      const metadata = parseStepJson("Config metadata", stepEditor.config_metadata_json);
      if (!metadata.ok) return false;
      next.parameters = {
        ...parameters,
        name: name.value,
        metadata: metadata.value
      };
    }
    if (next.kind === "subflow") {
      const subflowParameters = parseRequiredObject(stepEditor.subflow_parameters_json);
      if (!subflowParameters) {
        setStepEditorError("Subflow parameters must be a JSON object");
        return false;
      }
      if (!String(stepEditor.subflow_id ?? "").trim()) {
        setStepEditorError("Subflow workflow id is required");
        return false;
      }
      next.subflow_id = String(stepEditor.subflow_id ?? "").trim();
      next.parameters = subflowParameters;
    } else {
      delete next.subflow_id;
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
    if (!node) return;
    stepEditorHydrating = true;
    if (stepEditorApplyTimer) {
      clearTimeout(stepEditorApplyTimer);
      stepEditorApplyTimer = null;
    }
    selectedStepId.value = nodeId;
    stepEditor.id = nodeId;
    stepEditor.name = String(node.name ?? "");
    stepEditor.kind = node.kind ?? "action";
    stepEditor.approval_type = String(node.parameters?.approval_type ?? "generic");
    stepEditor.approval_prompt = String(node.parameters?.prompt ?? "Approval required");
    stepEditor.gate_kind = String(node.parameters?.kind ?? "manual");
    stepEditor.gate_when_json = pretty(node.parameters?.when ?? {});
    stepEditor.gate_poll_interval = Number(node.parameters?.poll_interval ?? 30);
    stepEditor.gate_timeout = Number(node.parameters?.timeout ?? 0);
    stepEditor.gate_label = String(node.parameters?.label ?? "");
    stepEditor.signal_name = String(node.parameters?.name ?? "signal");
    stepEditor.condition_fallback = nodeRefId(node.transitions?.next) ?? "";
    stepEditor.condition_branches = Array.isArray(node.transitions?.branches)
      ? node.transitions.branches.map((branch: JsonRecord) => ({ when_json: pretty(branch.when ?? {}), target: nodeRefId(branch.target) ?? "" }))
      : [];
    stepEditor.wait_seconds = Number(node.wait?.seconds ?? 60);
    stepEditor.wait_initial_status = String(node.wait?.initial_status ?? "waiting");
    stepEditor.wait_until_status = String(node.wait?.until_status ?? "");
    stepEditor.wait_json = pretty(node.wait ?? {});
    stepEditor.loop_items_json = pretty(node.parameters?.items ?? []);
    stepEditor.loop_target = nodeRefId(node.parameters?.target) ?? "";
    stepEditor.loop_max_iterations = Number(node.max_iterations ?? 10);
    stepEditor.switch_value_json = pretty(node.parameters?.value ?? valueRef("params", ["mode"]));
    stepEditor.switch_cases = Array.isArray(node.parameters?.cases)
      ? node.parameters.cases.map(switchCaseEditor)
      : [];
    stepEditor.switch_default = nodeRefId(node.parameters?.default) ?? "";
    stepEditor.parallel_branches = nodeRefArray(node.parameters?.branches);
    stepEditor.join_wait_for = nodeRefArray(node.parameters?.wait_for);
    stepEditor.join_mode = branchPolicyName(node.parameters?.mode, "all");
    stepEditor.try_body = nodeRefId(node.parameters?.body) ?? "";
    stepEditor.try_catch = nodeRefId(node.parameters?.catch) ?? "";
    stepEditor.try_finally = nodeRefId(node.parameters?.finally) ?? "";
    stepEditor.map_items_json = pretty(node.parameters?.items ?? []);
    stepEditor.map_target = nodeRefId(node.parameters?.target) ?? "";
    stepEditor.map_concurrency = Number(node.parameters?.concurrency ?? 1);
    stepEditor.race_branches = nodeRefArray(node.parameters?.branches);
    stepEditor.race_winner = branchPolicyName(node.parameters?.winner, "first_success");
    stepEditor.output_event_type = String(node.parameters?.event_type ?? "workflow.output");
    stepEditor.output_data_json = stepEditorJson(node.parameters?.data ?? null);
    stepEditor.input_prompt = String(node.parameters?.prompt ?? "Provide input");
    stepEditor.config_name_json = stepEditorJson(node.parameters?.name ?? "");
    stepEditor.config_metadata_json = stepEditorJson(node.parameters?.metadata ?? {});
    stepEditor.subflow_id = String(node.subflow_id ?? "");
    stepEditor.subflow_parameters_json = pretty(node.parameters ?? {});
    stepEditor.locked = isLockedWorkflowNode(node);
    stepEditor.skipped = node.skipped === true;
    stepEditor.max_attempts = Number(node.retry?.max_attempts ?? 1);
    stepEditor.timeout_seconds = Number(node.timeout_seconds ?? 0);
    const actionConfig = workflowNodeActionConfig(node);
    stepEditor.action_name = actionConfig.provider;
    stepEditor.action_function = actionConfig.action;
    // action nodes carry their inputs in action.configuration (merged with node.parameters); show the effective set.
    const actionInputs = node.kind === "action" ? workflowNodeActionInputs(node) : node.parameters ?? {};
    stepEditor.parameters_json = pretty(actionInputs);
    stepEditor.transitions_json = pretty(node.transitions ?? {});
    workflowInspectorMode.value = "step";
    updateSelectedWorkflowNodeDetail();
    setTimeout(() => {
      stepEditorHydrating = false;
    }, 0);
  }

  async function updateSelectedWorkflowNodeDetail() {
    selectedWorkflowNodeRunId.value = null;
    workflowNodeDetailExtra.value = "";
    const nodeId = selectedWorkflowRunNodeId.value || selectedStepId.value;
    const step = workflowRunDetail.value?.nodes.find((node) => node.node_id === nodeId);
    if (!step?.id) return;
    selectedWorkflowNodeRunId.value = step.id;
    const [nodeChunks, nodeArtifacts] = await Promise.all([
      app.runOperation("Loading node chunks", () => fetchWorkflowNodeRunChunks(step.id)).catch(() => [] as RunChunk[]),
      app.runOperation("Loading node artifacts", () => fetchWorkflowNodeRunArtifacts(step.id)).catch(() => [] as RunArtifact[])
    ]);
    workflowNodeDetailExtra.value = [
      "",
      `Workflow node run ${step.id} chunks`,
      ...nodeChunks.map((chunk) => `[${chunk.stream}] ${chunk.content}`),
      "",
      `Workflow node run ${step.id} artifacts`,
      ...nodeArtifacts.map((artifact) => `${artifact.name} (${artifact.size_bytes} bytes) ${artifact.uri}`)
    ].join("\n");
  }

  function workflowNameForRun(run: RunSummary): string {
    return workflows.value.find((workflow) => workflow.id === run.workflow_id)?.name ?? "";
  }

  function onGraphNodeClick(event: any) {
    const nodeId = event?.node?.id;
    if (nodeId) {
      dismissStepEditorForCanvasEdit();
      selectedGraphEdgeId.value = "";
      populateStepEditor(nodeId);
    }
  }

  function onGraphNodeDoubleClick(event: any) {
    const nodeId = event?.node?.id;
    if (nodeId) openStepEditor(nodeId, false);
  }

  function onGraphNodeDragStop(event: any) {
    const node = event?.node;
    if (!node?.id) return;
    dismissStepEditorForCanvasEdit();
    setGraphNodePosition(node.id, node.position);
    syncWorkflowDraftToJson();
  }

  function onGraphNodesChange(changes: any[]) {
    let changed = false;
    for (const change of changes) {
      if (change.type !== "position" || !change.id || !change.position || change.dragging) continue;
      setGraphNodePosition(change.id, change.position);
      changed = true;
    }
    if (changed) syncWorkflowDraftToJson();
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
    const previousEdge = draft.edgeId ? graphEdges.value.find((edge: Edge) => edge.id === draft.edgeId) ?? null : null;
    const result = applyWorkflowEdgeEditorDraft(workflowDraft.definition, previousEdge, draft);
    if (!result.ok) {
      app.setError(result.message);
      return false;
    }
    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    return true;
  }

  function moveEdgeEditorItem(draft: WorkflowEdgeEditorDraft, direction: -1 | 1): WorkflowEdgeEditorDraft | null {
    const result = moveWorkflowEdgeEditorDraft(workflowDraft.definition, draft, direction);
    if (!result.ok) {
      app.setError(result.message);
      return null;
    }
    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    const movedEdge = graphEdges.value.find((edge: Edge) =>
      edge.source === result.draft.source &&
      edge.target === result.draft.target &&
      workflowEdgeOptionId(edge) === result.draft.optionId
    );
    return movedEdge ? { ...result.draft, edgeId: movedEdge.id } : result.draft;
  }

  function moveSelectedEdge(direction: -1 | 1): boolean {
    const draft = selectedGraphEdgeId.value ? openEdgeEditorDraft(selectedGraphEdgeId.value) : null;
    if (!draft) return false;
    const moved = moveEdgeEditorItem(draft, direction);
    if (!moved) return false;
    selectedGraphEdgeId.value = moved.edgeId;
    return true;
  }

  function reverseSelectedEdgeHandles(): boolean {
    const edge = selectedGraphEdge.value;
    if (!edge) return false;
    dismissStepEditorForCanvasEdit();
    const data = edge.data as any;
    const semanticKey = data?.transitionKey ?? (typeof data?.branchIndex === "number" ? `branches.${data.branchIndex}` : parameterSemanticKey(data?.parameterKey, data?.parameterIndex));
    setWorkflowEdgeHandles(workflowDraft.definition, edge.source, semanticKey, edge.targetHandle, edge.sourceHandle, data?.edgeStyle);
    syncWorkflowDraftToJson();
    selectedGraphEdgeId.value = "";
    return true;
  }

  function setEdgeLabelOffset(edgeId: string, offset: { x: number; y: number } | null): boolean {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);
    if (!edge) return false;
    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelOffset(workflowDraft.definition, edge, offset);
    syncWorkflowDraftToJson();
    return true;
  }

  function setEdgeLabelAnchor(edgeId: string, position: number | null): boolean {
    const edge = graphEdges.value.find((item: Edge) => item.id === edgeId);
    if (!edge) return false;
    dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelAnchor(workflowDraft.definition, edge, position === null ? null : { position });
    syncWorkflowDraftToJson();
    return true;
  }

  function scheduleStepEditorApply() {
    void applyStepEditor();
  }

  function applyGraphEdgeSemantic(connection: any, optionId: string, previousEdgeId = ""): boolean {
    const { source, target, sourceHandle } = connection;
    if (!source || !target) return false;
    dismissStepEditorForCanvasEdit();
    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return false;
    }
    const previousEdge = previousEdgeId ? graphEdges.value.find((edge: Edge) => edge.id === previousEdgeId) ?? null : null;
    const previousDraft = previousEdge ? workflowEdgeEditorDraft(workflowDraft, previousEdge) : null;
    const draft: WorkflowEdgeEditorDraft = {
      ...(previousDraft ?? defaultEdgeEditorDraft()),
      edgeId: previousEdgeId,
      source,
      target,
      optionId,
      sourceHandle,
      targetHandle: connection.targetHandle
    };
    return applyEdgeEditorDraft(draft);
  }

  function onGraphConnect(connection: any) {
    const source = connection?.source;
    const handleOptionId = optionIdForSourceHandle(connection?.sourceHandle);
    const options = source ? workflowEdgeOptions(source) : [];
    if (!source || options.length === 0) return;
    const optionId = handleOptionId && options.some((option) => option.id === handleOptionId) ? handleOptionId : options.length === 1 ? options[0].id : "";
    if (optionId) applyGraphEdgeSemantic(connection, optionId);
  }

  function onGraphEdgeClick(event: any) {
    const edgeId = event?.edge?.id;
    if (edgeId) selectGraphEdge(edgeId);
  }

  function onGraphEdgeUpdate(event: any) {
    const edge = event?.edge;
    const connection = event?.connection;
    if (!edge || !connection?.source || !connection?.target) return;
    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return;
    }
    const optionId = workflowEdgeOptionId(edge);
    if (!optionId) return;
    if (applyGraphEdgeSemantic(connection, optionId, edge.id) && selectedStepId.value === edge.source) {
      populateStepEditor(edge.source);
    }
    selectedGraphEdgeId.value = "";
  }

  function onGraphEdgesChange(changes: any[]) {
    let changed = false;
    for (const change of changes) {
      if (change.type === "remove") {
        const edge = graphEdges.value.find((item: Edge) => item.id === change.id);
        if (edge) {
          const sourceNode = ensureWorkflowNodes().find((n: JsonRecord) => n.id === edge.source);
          if (sourceNode && removeWorkflowEdge(sourceNode, edge)) {
            const data = edge.data as any;
            if (data?.transitionKey) removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
            if (typeof data?.branchIndex === "number") removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, `branches.${data.branchIndex}`);
            if (data?.parameterKey) removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, parameterSemanticKey(data.parameterKey, data.parameterIndex));
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
    if (!edge) return;
    const sourceNode = ensureWorkflowNodes().find((node: JsonRecord) => node.id === edge.source);
    if (!sourceNode || !removeWorkflowEdge(sourceNode, edge)) return;
    const data = edge.data as any;
    if (data?.transitionKey) removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
    if (typeof data?.branchIndex === "number") removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, `branches.${data.branchIndex}`);
    if (data?.parameterKey) removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, parameterSemanticKey(data.parameterKey, data.parameterIndex));
    syncWorkflowDraftToJson();
    if (selectedStepId.value) populateStepEditor(selectedStepId.value);
  }

  function autoArrangeWorkflowNodes(direction: WorkflowLayoutDirection = workflowLayoutDirection.value) {
    if (!syncWorkflowJson()) return;
    workflowLayoutDirection.value = direction;
    const positions = autoArrangeWorkflowLayout(workflowDraft.definition, direction);
    for (const [nodeId, position] of Object.entries(positions)) setGraphNodePosition(nodeId, position);
    autoArrangeWorkflowEdgeHandles(workflowDraft.definition, positions);
    workflowLayoutVersion.value += 1;
    syncWorkflowDraftToJson();
  }

  function scheduleWorkflowJsonSync() {
    void syncWorkflowJson();
  }

  function scheduleWorkflowWdlSync() {
    void syncWorkflowWdl();
  }

  function scheduleWorkflowWdlRefresh() {
    void refreshWorkflowWdl();
  }

  function setWorkflowJsonSilently(next: string) {
    if (workflowJsonWriteReleaseTimer) clearTimeout(workflowJsonWriteReleaseTimer);
    workflowJsonWriteGuard = true;
    workflowJson.value = next;
    workflowJsonWriteReleaseTimer = setTimeout(() => {
      workflowJsonWriteGuard = false;
      workflowJsonWriteReleaseTimer = null;
    }, 0);
  }

  function setWorkflowWdlSilently(next: string) {
    if (workflowWdlWriteReleaseTimer) clearTimeout(workflowWdlWriteReleaseTimer);
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
    let compiled: WorkflowDefinition;
    const previousUi = isJsonObject(workflowDraft.definition?.ui) ? cloneJson(workflowDraft.definition.ui) : null;
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
    if (previousUi) workflowDraft.definition.ui = previousUi;
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
      const name = workflowDraft.name?.trim() || "workflow";
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
    const allWorkflows = workflows.value.filter((workflow) => workflow.id != null);
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
        let slug = (workflow.name?.trim() || `workflow-${workflow.id}`).replace(/[^a-z0-9._-]+/gi, "_");
        while (usedNames.has(slug)) {
          slug = `${slug}_${workflow.id}`;
        }
        usedNames.add(slug);
        const fileName = `${slug}.wdl`;
        entries.push({ name: fileName, content: source });
        manifestWorkflows.push(fileName);
        triggers.push(...(await fetchWorkflowTriggers(workflow.id!).catch(() => [])));
      }
      if (entries.length === 0) {
        throw new Error("no workflows could be decompiled to WDL");
      }
      const manifest = { version: 1, workflows: manifestWorkflows, triggers };
      entries.unshift({ name: "pack.wdlp", content: pretty(manifest) });
      downloadBlob("runinator-pack.zip", createZip(entries));
      const note = skipped.length ? ` (skipped ${skipped.length} non-WDL: ${skipped.join(", ")})` : "";
      app.setStatus(`Exported ${entries.length - 1} workflow(s) to runinator-pack.zip${note}`);
    });
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!workflowDraft.definition || typeof workflowDraft.definition !== "object") workflowDraft.definition = {};
    if (!Array.isArray(workflowDraft.definition.nodes)) workflowDraft.definition.nodes = [];
    return workflowDraft.definition.nodes;
  }

  function stripNewNodeConnections(node: JsonRecord) {
    const transitions = node.transitions && typeof node.transitions === "object" && !Array.isArray(node.transitions) ? node.transitions : {};
    for (const key of directTransitionKeys) {
      delete transitions[key];
    }
    delete transitions.branches;
    node.transitions = transitions;

    const parameters = node.parameters && typeof node.parameters === "object" && !Array.isArray(node.parameters) ? node.parameters : {};
    delete parameters.target;
    delete parameters.default;
    delete parameters.body;
    delete parameters.catch;
    delete parameters.finally;
    if (Array.isArray(parameters.cases)) parameters.cases = [];
    if (Array.isArray(parameters.branches)) parameters.branches = [];
    if (Array.isArray(parameters.wait_for)) parameters.wait_for = [];
    node.parameters = parameters;
  }

  function graphCentroidPosition(): { x: number; y: number } {
    const positioned = graphNodes.value
      .map((node) => ({
        x: Number(node.position?.x),
        y: Number(node.position?.y)
      }))
      .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));
    if (positioned.length === 0) return nextNodePosition(1);
    const totals = positioned.reduce(
      (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
      { x: 0, y: 0 }
    );
    return {
      x: Math.round(totals.x / positioned.length),
      y: Math.round(totals.y / positioned.length)
    };
  }

  function moveWorkflowSelection(delta: number) {
    const list = filteredWorkflows.value;
    if (list.length === 0) return;
    const current = list.findIndex((workflow) => workflow.id === selectedWorkflowId.value);
    selectWorkflow(list[boundedIndex(current, delta, list.length)]);
  }

  function setGraphNodePosition(nodeId: string, position: { x: number; y: number }) {
    const definition = workflowDraft.definition;
    definition.ui = definition.ui ?? {};
    definition.ui.layout = definition.ui.layout ?? {};
    definition.ui.layout.nodes = definition.ui.layout.nodes ?? {};
    definition.ui.layout.nodes[nodeId] = { x: Number(position.x ?? 0), y: Number(position.y ?? 0) };
  }

  function renameLayoutNode(previousId: string, nextId: string) {
    if (!previousId || previousId === nextId) return;
    const layout = workflowDraft.definition?.ui?.layout?.nodes;
    if (!layout?.[previousId]) return;
    layout[nextId] = layout[previousId];
    delete layout[previousId];
  }

  function addConditionBranchEditor() {
    stepEditor.condition_branches.push({ when_json: pretty({ value: valueRef("params", ["value"]), equals: true }), target: "" });
    markWorkflowDirty();
  }

  function removeConditionBranchEditor(index: number) {
    stepEditor.condition_branches.splice(index, 1);
    const node = selectedNode.value;
    if (node?.kind === "condition") removeConditionBranch(node, index);
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
    if (!workflowDraft.id) {
      workflowTriggers.value = [];
      closeTriggerEditor();
      return;
    }
    workflowTriggers.value = await app
      .runOperation("Loading workflow triggers", () => fetchWorkflowTriggers(workflowDraft.id!))
      .catch(() => []);
  }

  function clearWorkflowTriggerState() {
    workflowTriggers.value = [];
    closeTriggerEditor();
  }

  function addWorkflowTrigger(kind: WorkflowTriggerKind = "cron") {
    if (!workflowDraft.id) return;
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
    triggerJson.configuration = pretty(trigger.configuration ?? {});
    triggerJson.metadata = pretty(trigger.metadata ?? {});
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
    if (!workflowDraft.id) return;
    const configuration = parseRequiredObject(triggerJson.configuration);
    const metadata = parseRequiredObject(triggerJson.metadata);
    if (!configuration || !metadata) {
      triggerEditorError.value = configuration ? "Trigger metadata must be a JSON object" : "Trigger configuration must be a JSON object";
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
      blackout_end: dateTimeLocalToIso(triggerDraft.blackout_end)
    };
    const saved = await app.runOperation("Saving workflow trigger", () => saveWorkflowTrigger(trigger, triggerEditorCreating.value));
    app.setStatus(`Workflow trigger saved: ${saved.kind}`);
    closeTriggerEditor();
    await refreshWorkflowTriggers();
  }

  async function deleteSelectedWorkflowTrigger(trigger: WorkflowTrigger) {
    if (!trigger.id) return;
    if (!window.confirm(`Delete ${trigger.kind} trigger ${trigger.id}?`)) return;
    const response = await app.runOperation("Deleting workflow trigger", () => deleteWorkflowTrigger(trigger.id!));
    if (response.success === false) {
      app.setError(response.message || "Failed to delete workflow trigger");
      return;
    }
    app.setStatus(response.message || "Workflow trigger deleted");
    if (triggerDraft.id === trigger.id) closeTriggerEditor();
    await refreshWorkflowTriggers();
  }

  function triggerCronSummary(trigger: WorkflowTrigger): string {
    const cron = trigger.configuration?.cron;
    return typeof cron === "string" && cron.trim() ? cron : "";
  }

  function triggerDateForInput(value: string | null | undefined): string {
    if (!value) return "";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
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
    stepEditorOpen.value = true;
  }

  async function submitStepEditor() {
    if (!applyStepEditor()) return;
    stepEditorOpen.value = false;
    stepEditorCreating.value = false;
    stepEditorCreatedNodeId.value = "";
    selectedStepId.value = "";
    // applying a step persists the workflow so canvas edits do not need a manual save.
    await saveSelectedWorkflowBundle();
  }

  // dismiss an open node editor when the user starts editing on the canvas instead.
  function dismissStepEditorForCanvasEdit() {
    if (!stepEditorOpen.value || stepEditorCreating.value) return;
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
      workflowDraft.definition.nodes = ensureWorkflowNodes().filter((node: JsonRecord) => node.id !== nodeId);
      syncWorkflowDraftToJson();
    } else if (stepEditorBaselineDefinition) {
      workflowDraft.definition = cloneJson(stepEditorBaselineDefinition);
      syncWorkflowDraftToJson();
    }
    selectedStepId.value = "";
    stepEditorOpen.value = false;
    stepEditorCreating.value = false;
    stepEditorCreatedNodeId.value = "";
    stepEditorError.value = "";
    stepEditorBaselineDefinition = null;
    stepEditorHydrating = false;
  }

  function duplicateSelectedStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) return;
    const nodes = ensureWorkflowNodes();
    const source = nodes.find((node: JsonRecord) => node.id === selectedStepId.value);
    if (!source) return;
    const copy = cloneJson(source);
    copy.id = uniqueWorkflowNodeId(nodes, `${source.id}_copy`);
    stripNewNodeConnections(copy);
    const position = graphCentroidPosition();
    nodes.push(copy);
    setGraphNodePosition(copy.id, position);
    syncWorkflowDraftToJson();
    populateStepEditor(copy.id);
    openStepEditor(copy.id, true);
  }

  function setStepEditorError(message: string) {
    stepEditorError.value = message;
    app.setError(message);
  }

  function workflowSaveTriggers(workflowId: string | null | undefined): WorkflowTrigger[] {
    if (workflowId == null) return [];
    return workflowTriggers.value
      .filter((trigger) => trigger.workflow_id === workflowId)
      .map((trigger) => cloneJson(trigger));
  }

  async function workflowWdlSaveRequest(): Promise<WorkflowWdlSaveRequest> {
    const workflow = cloneJson(workflowDraft);
    const workflowId = workflow.id ?? null;
    const source = await decompileToWdl(workflow);
    const triggers = workflowId === null
      ? []
      : workflowSaveTriggers(workflowId);
    const request: WorkflowWdlSaveRequest = {
      source,
      enabled: workflow.enabled,
      workflow_id: workflowId,
      triggers
    };
    if (isJsonObject(workflow.definition?.ui)) request.ui = cloneJson(workflow.definition.ui);
    return request;
  }

  function parseStepJson(label: string, text: string): { ok: true; value: any } | { ok: false } {
    const value = parseRequiredJson(text);
    if (value !== null || text.trim() === "null") return { ok: true, value };
    setStepEditorError(`${label} must be valid JSON`);
    return { ok: false };
  }

  function stepEditorJson(value: unknown): string {
    return JSON.stringify(value === undefined ? null : value, null, 2);
  }

  function isJsonObject(value: unknown): value is JsonRecord {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }

  function validateStepParameters(parameters: JsonRecord): string {
    if (stepEditor.kind !== "action") return "";
    const provider = useProvidersStore().providers.find((item) => item.name === stepEditor.action_name);
    const action = provider?.actions.find((item) => item.function_name === stepEditor.action_function);
    if (!action) return "Select a valid task provider action";
    for (const parameter of action.parameters ?? []) {
      if (!parameter.required) continue;
      const value = parameters[parameter.name];
      if (isBlankValue(value)) {
        return `${parameter.label || parameter.name} is required`;
      }
      const typeError = validateJsonValueType(value, parameter.ty, parameter.label || parameter.name);
      if (typeError) return typeError;
    }
    return "";
  }

  async function saveSelectedWorkflowBundle() {
    const synced = workflowEditorMode.value === "wdl" ? await syncWorkflowWdl() : syncWorkflowJson();
    if (!synced) return;
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    const saved = await app.runOperation("Saving workflow", async () => saveWorkflowWdl(await workflowWdlSaveRequest()));
    const savedWorkflow = saved.workflows[0];
    if (!savedWorkflow) {
      app.setError("Workflow bundle save returned no workflow");
      return;
    }
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(savedWorkflow)));
    workflowTriggers.value = saved.triggers.filter((trigger) => trigger.workflow_id === workflowDraft.id);
    setWorkflowJsonSilently(pretty(workflowDraft.definition));
    scheduleWorkflowWdlRefresh();
    app.setStatus(`Workflow saved: ${savedWorkflow.name}`);
    isDirty.value = false;
    selectedWorkflowId.value = savedWorkflow.id;
    await refreshWorkflows();
  }

  async function deleteSelectedWorkflow() {
    const workflow = selectedWorkflow.value;
    if (!workflow?.id) return;
    if (
      !window.confirm(
        `Delete workflow "${workflow.name}"?\n\nThis permanently deletes the workflow along with ALL of its runs and their execution history. This cannot be undone.`,
      )
    )
      return;
    const response = await app.runOperation(`Deleting workflow ${workflow.name}`, () => deleteWorkflow(workflow.id!));
    if (response.success === false) {
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
    if (!workflow?.id) return;
    if (isDirty.value) {
      app.setError("Save or discard the current changes before duplicating this workflow.");
      return;
    }
    const copy = await app
      .runOperation(`Duplicating workflow ${workflow.name}`, () => duplicateWorkflow(workflow.id!, bump))
      .catch((error) => {
        app.setError(error instanceof Error ? error.message : "Failed to duplicate workflow");
        return null;
      });
    if (!copy) return;
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
    workflowNodeDetailExtra,
    selectedStepId,
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
    addSwitchCaseEditor,
    removeSwitchCaseEditor,
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
    markWorkflowDirty
  };
});
