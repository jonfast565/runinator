import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  createWorkflowRun,
  deleteWorkflow,
  deleteWorkflowTrigger,
  fetchWorkflowNodeRunArtifacts,
  fetchWorkflowNodeRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  fetchWorkflowTriggers,
  fetchWorkflows,
  saveWorkflowBundle,
  saveWorkflowTrigger,
  stepWorkflowRun
} from "../api/commandCenterApi";
import type { Edge } from "@vue-flow/core";
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, ScheduledTask, WorkflowDefinition, WorkflowLayoutDirection, WorkflowNodeKind, WorkflowRunDetail, WorkflowTrigger, WorkflowTriggerKind } from "../types/models";
import { pretty } from "../utils/format";
import { cloneJson, parseObject, parseRequiredJson, parseRequiredObject } from "../utils/json";
import {
  addDirectTransition,
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
  uniqueWorkflowNodeId,
  validateWorkflowReferenceSyntax,
  valueRef,
  workflowNodeActionConfig,
  workflowNodeKinds,
  workflowRunSearchText
} from "../utils/workflows";
import { useAppStore } from "./app";
import { useProvidersStore } from "./providers";
import { useResourcesStore } from "./resources";

type BranchPolicyName = "all" | "any" | "first_success";
type SwitchCaseEditor = { match_kind: "equals" | "not_equals" | "exists" | "when"; match_json: string; target: string };

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<number | null>(null);
  const workflowDraft = reactive<WorkflowDefinition>(newWorkflowDraft());
  const workflowJson = ref("{}");
  const workflowConcurrency = ref(1);
  const workflowSettingsOpen = ref(false);
  const workflowTriggers = ref<WorkflowTrigger[]>([]);
  const triggerEditorOpen = ref(false);
  const triggerEditorCreating = ref(false);
  const triggerEditorError = ref("");
  const triggerDraft = reactive<WorkflowTrigger>(newWorkflowTriggerDraft(0, "cron"));
  const triggerJson = reactive({ configuration: "{}", metadata: "{}" });
  const workflowEditorMode = ref<"graph" | "json">("graph");
  const workflowLayoutDirection = ref<WorkflowLayoutDirection>("horizontal");
  const workflowInspectorMode = ref<"step">("step");
  const stepEditorOpen = ref(false);
  const stepEditorCreating = ref(false);
  const stepEditorCreatedNodeId = ref("");
  const stepEditorError = ref("");
  const workflowRuns = ref<RunSummary[]>([]);
  const workflowLayoutVersion = ref(0);
  const workflowTaskDrafts = ref<Record<string, ScheduledTask>>({});
  const workflowRunsByRunId = computed(() => {
    const groups: Record<number, RunSummary[]> = {};
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
  const selectedWorkflowRunId = ref(0);
  const workflowRunDetail = ref<WorkflowRunDetail | null>(null);
  const workflowNodeDetailExtra = ref("");
  const selectedStepId = ref("");
  const selectedWorkflowRunNodeId = ref("");
  const selectedWorkflowNodeRunId = ref(0);
  const stepEditor = reactive({
    id: "",
    kind: "task" as WorkflowNodeKind,
    approval_type: "generic",
    approval_prompt: "Approval required",
    condition_fallback: "",
    condition_branches: [] as Array<{ when_json: string; target: string }>,
    wait_seconds: 60,
    wait_initial_status: "waiting",
    wait_until_status: "",
    wait_json: "{}",
    loop_items_json: "[]",
    loop_target: "",
    loop_max_iterations: 10,
    switch_value_json: pretty(valueRef("input", ["mode"])),
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
    emit_event_type: "workflow.event",
    emit_data_json: "{}",
    subflow_id: 0,
    subflow_parameters_json: "{}",
    max_attempts: 1,
    task_id: 1,
    timeout_seconds: 0,
    action_name: "",
    action_function: "",
    parameters_json: "{}",
    transitions_json: "{}"
  });

  const isDirty = ref(false);

  const app = useAppStore();
  const selectedWorkflow = computed(() => workflows.value.find((workflow) => workflow.id === selectedWorkflowId.value) ?? null);
  const canRunWorkflow = computed(() => Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id));
  const canManageWorkflowTriggers = computed(() => Boolean(workflowDraft.id));
  const canStepWorkflowRun = computed(() => workflowRunDetail.value?.run.status === "debug_paused");
  const canRemoveSelectedStep = computed(() => {
    const node = workflowDraft.definition?.nodes?.find((item: JsonRecord) => item.id === selectedStepId.value);
    return Boolean(node && node.kind !== "start" && node.kind !== "end" && node.kind !== "fail");
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
  const graphNodes = computed(() => buildGraphNodes(workflowDraft, null, Object.values(workflowTaskDrafts.value)));
  const graphEdges = computed(() => buildGraphEdges(workflowDraft));
  const workflowRunWorkflow = computed(() => {
    const workflowId = workflowRunDetail.value?.run.workflow_id ?? workflowRuns.value.find((run) => run.id === selectedWorkflowRunId.value)?.workflow_id;
    return workflows.value.find((workflow) => workflow.id === workflowId) ?? null;
  });
  const runGraphNodes = computed(() => workflowRunWorkflow.value
    ? buildGraphNodes(workflowRunWorkflow.value, workflowRunDetail.value, Object.values(workflowTaskDrafts.value)).map((node) => ({
        ...node,
        data: { ...(node.data as JsonRecord), readOnly: true }
      }))
    : []);
  const runGraphEdges = computed(() => workflowRunWorkflow.value ? buildGraphEdges(workflowRunWorkflow.value) : []);
  const selectedNode = computed(() => ensureWorkflowNodes().find((item: JsonRecord) => item.id === selectedStepId.value) ?? null);
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
    workflowJson.value = pretty(workflowDraft.definition ?? { nodes: [] });
    if (isSwitch) {
      selectedStepId.value = "";
      workflowTaskDrafts.value = {};
      clearWorkflowTriggerState();
      stepEditorOpen.value = false;
    }
    isDirty.value = false;
    return Promise.resolve();
  }

  function addWorkflow() {
    const workflow = newWorkflowDraft();
    workflows.value.push(workflow);
    selectWorkflow(workflow);
  }

  async function runSelectedWorkflow(debug = false) {
    const workflow = selectedWorkflow.value;
    if (!workflow?.id || !workflow.enabled) return app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
    const response = await app.runOperation(
      debug ? `Running workflow ${workflow.name} in debug mode` : `Running workflow ${workflow.name}`,
      () => createWorkflowRun(workflow.id!, { debug })
    );
    selectedWorkflowRunId.value = response.id;
    app.setStatus(`${debug ? "Debug workflow run" : "Workflow run"} queued: ${response.id}`);
    await fetchWorkflowRunDetail(response.id);
    await fetchRecentWorkflowRuns();
    app.activeTab = "Runs";
  }

  async function runSelectedWorkflowDebug() {
    return runSelectedWorkflow(true);
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

  async function fetchWorkflowRunsForSelected(workflowId: number) {
    console.info("[command-center] refreshing workflow runs", { workflowId });
    workflowRuns.value = await app.runOperation("Loading workflow runs", () => fetchWorkflowRuns(workflowId)).catch(() => []);
    if (!workflowRuns.value.some((run) => run.id === selectedWorkflowRunId.value)) {
      selectedWorkflowRunId.value = workflowRuns.value[0]?.id ?? 0;
    }
  }

  async function fetchRecentWorkflowRuns() {
    console.info("[command-center] refreshing recent workflow runs");
    workflowRuns.value = await app.runOperation("Loading workflow runs", () => fetchWorkflowRuns()).catch(() => []);
    const previousRunId = selectedWorkflowRunId.value;
    if (!workflowRuns.value.some((run) => run.id === selectedWorkflowRunId.value)) {
      selectedWorkflowRunId.value = workflowRuns.value[0]?.id ?? 0;
    }
    if (selectedWorkflowRunId.value > 0 && (!workflowRunDetail.value || previousRunId !== selectedWorkflowRunId.value)) {
      await fetchWorkflowRunDetail(selectedWorkflowRunId.value, true);
    }
  }

  async function selectWorkflowRun(run: RunSummary) {
    selectedWorkflowRunId.value = run.id;
    return fetchWorkflowRunDetail(run.id);
  }

  async function fetchWorkflowRunDetail(workflowRunId: number, silent = false) {
    console.info("[command-center] refreshing workflow run detail", { workflowRunId, silent });
    const detail = silent
      ? await fetchWorkflowRun(workflowRunId).catch(() => null)
      : await app.runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId)).catch(() => null);
    applyWorkflowRunDetail(detail);
  }

  function setWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    applyWorkflowRunDetail(detail);
  }

  function selectWorkflowRunNode(nodeId: string) {
    selectedWorkflowRunNodeId.value = nodeId;
    updateSelectedWorkflowNodeDetail();
  }

  function applyWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    workflowRunDetail.value = detail;
    workflowNodeDetailExtra.value = "";
    if (!detail?.nodes.some((node) => node.node_id === selectedWorkflowRunNodeId.value)) {
      selectedWorkflowRunNodeId.value = detail?.nodes[0]?.node_id ?? "";
    }
    if (detail) {
      const resources = useResourcesStore();
      const hasWaiting = detail.nodes.some(n =>
        n.status === "waiting" || n.status === "approval_required" || n.status === "pending"
      );
      if (hasWaiting) resources.refreshResources();
    }
  }

  async function addWorkflowStep() {
    await addWorkflowNode("task");
  }

  async function addWorkflowNode(kind: WorkflowNodeKind) {
    if (!syncWorkflowJson()) return;
    const nodes = ensureWorkflowNodes();
    const taskId = 1;
    const newNode = createWorkflowNode(kind, nodes, taskId);
    const endNode = nodes.find((node: JsonRecord) => node.kind === "end");
    if (endNode?.id) normalizeNewNodeTargets(newNode, endNode.id);
    const endIndex = nodes.findIndex((node: JsonRecord) => node.kind === "end");
    if (endIndex >= 0) nodes.splice(endIndex, 0, newNode);
    else nodes.push(newNode);
    const startNode = nodes.find((node: JsonRecord) => node.kind === "start");
    if (startNode) {
      startNode.transitions = startNode.transitions ?? {};
      if (!startNode.transitions.next || nodeRefId(startNode.transitions.next) === endNode?.id) startNode.transitions.next = nodeRef(newNode.id);
    }
    setGraphNodePosition(newNode.id, nextNodePosition(nodes.length));
    syncWorkflowDraftToJson();
    populateStepEditor(newNode.id);
    openStepEditor(newNode.id, true);
  }

  function removeWorkflowStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) return;
    removeWorkflowNode(selectedStepId.value);
  }

  function removeWorkflowNode(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);
    if (!node || node.kind === "start" || node.kind === "end" || node.kind === "fail") return;
    workflowDraft.definition.nodes = ensureWorkflowNodes().filter((item: JsonRecord) => item.id !== nodeId);
    removeWorkflowNodeReferences(workflowDraft.definition, nodeId);
    delete workflowDraft.definition.ui?.layout?.nodes?.[nodeId];
    delete workflowTaskDrafts.value[nodeId];
    if (selectedStepId.value === nodeId) selectedStepId.value = "";
    syncWorkflowDraftToJson();
  }

  function applyStepEditor(): boolean {
    stepEditorError.value = "";
    if (!selectedStepId.value) return false;
    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === selectedStepId.value);
    if (index < 0) return false;
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
    next.kind = stepEditor.kind;
    if (next.kind === "task" || next.kind === "action") {
      if (next.kind === "action") {
        next.action = {
          ...(typeof next.action === "object" && next.action ? next.action : {}),
          provider: stepEditor.action_name,
          function: stepEditor.action_function,
          timeout_seconds: stepEditor.timeout_seconds > 0 ? stepEditor.timeout_seconds : next.action?.timeout_seconds ?? 300,
          configuration: next.action?.configuration ?? {}
        };
        delete next.action_name;
        delete next.action_function;
      } else {
        next.action_name = stepEditor.action_name;
        next.action_function = stepEditor.action_function;
      }
    } else {
      delete next.action;
      delete next.action_name;
      delete next.action_function;
    }
    next.retry = { max_attempts: stepEditor.max_attempts };
    if (stepEditor.timeout_seconds > 0) next.timeout_seconds = stepEditor.timeout_seconds;
    else delete next.timeout_seconds;
    next.parameters = parameters;
    next.transitions = transitions;
    if (next.kind === "approval") {
      next.parameters = { ...parameters, approval_type: stepEditor.approval_type || "generic", prompt: stepEditor.approval_prompt || "Approval required" };
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
    if (next.kind === "emit") {
      const data = parseStepJson("Emit data", stepEditor.emit_data_json);
      if (!data.ok) return false;
      next.parameters = {
        ...parameters,
        event_type: stepEditor.emit_event_type.trim() || "workflow.event",
        data: data.value
      };
    }
    if (next.kind === "subflow") {
      const subflowParameters = parseRequiredObject(stepEditor.subflow_parameters_json);
      if (!subflowParameters) {
        setStepEditorError("Subflow parameters must be a JSON object");
        return false;
      }
      next.subflow_id = Math.max(0, Number(stepEditor.subflow_id ?? 0));
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
    selectedStepId.value = nodeId;
    stepEditor.id = nodeId;
    stepEditor.kind = node.kind ?? "task";
    stepEditor.task_id = Number(node.task_id ?? 1);
    stepEditor.approval_type = String(node.parameters?.approval_type ?? "generic");
    stepEditor.approval_prompt = String(node.parameters?.prompt ?? "Approval required");
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
    stepEditor.switch_value_json = pretty(node.parameters?.value ?? valueRef("input", ["mode"]));
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
    stepEditor.emit_event_type = String(node.parameters?.event_type ?? "workflow.event");
    stepEditor.emit_data_json = pretty(node.parameters?.data ?? {});
    stepEditor.subflow_id = Number(node.subflow_id ?? 0);
    stepEditor.subflow_parameters_json = pretty(node.parameters ?? {});
    stepEditor.max_attempts = Number(node.retry?.max_attempts ?? 1);
    stepEditor.timeout_seconds = Number(node.timeout_seconds ?? 0);
    const actionConfig = workflowNodeActionConfig(node);
    stepEditor.action_name = actionConfig.provider;
    stepEditor.action_function = actionConfig.action;
    stepEditor.parameters_json = pretty(node.parameters ?? {});
    stepEditor.transitions_json = pretty(node.transitions ?? {});
    workflowInspectorMode.value = "step";
    updateSelectedWorkflowNodeDetail();
  }

  async function updateSelectedWorkflowNodeDetail() {
    selectedWorkflowNodeRunId.value = 0;
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
    if (nodeId) populateStepEditor(nodeId);
  }

  function onGraphNodeDoubleClick(event: any) {
    const nodeId = event?.node?.id;
    if (nodeId) openStepEditor(nodeId, false);
  }

  function onGraphNodeDragStop(event: any) {
    const node = event?.node;
    if (!node?.id) return;
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

  function onGraphConnect(connection: any) {
    const { source, target, sourceHandle } = connection;
    if (!source || !target) return;
    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return;
    }
    const nodes = ensureWorkflowNodes();
    const sourceNode = nodes.find((n: JsonRecord) => n.id === source);
    if (!sourceNode) return;
    const transitionKey = addDirectTransition(sourceNode, target, sourceHandle);
    setWorkflowEdgeHandles(workflowDraft.definition, source, transitionKey, connection.sourceHandle, connection.targetHandle);
    syncWorkflowDraftToJson();
    if (selectedStepId.value === source) {
      populateStepEditor(source);
    }
  }

  function onGraphEdgeUpdate(event: any) {
    const edge = event?.edge;
    const connection = event?.connection;
    if (!edge || !connection?.source || !connection?.target) return;
    if (isSameConnectionPointLoop(connection)) {
      app.setError("Cannot connect a node handle back to itself");
      return;
    }
    const data = edge.data as any;
    if (!data?.editable) return;
    const nodes = ensureWorkflowNodes();
    const oldSourceNode = nodes.find((node: JsonRecord) => node.id === edge.source);
    const newSourceNode = nodes.find((node: JsonRecord) => node.id === connection.source);
    if (!newSourceNode) return;

    if (data.kind === "direct" && data.transitionKey) {
      if (oldSourceNode && edge.source !== connection.source) {
        delete oldSourceNode.transitions?.[data.transitionKey];
        removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
      }
      const transitionKey =
        edge.source === connection.source
          ? data.transitionKey
          : addDirectTransition(newSourceNode, connection.target, data.transitionKey);
      newSourceNode.transitions = newSourceNode.transitions ?? {};
      newSourceNode.transitions[transitionKey] = nodeRef(connection.target);
      setWorkflowEdgeHandles(workflowDraft.definition, connection.source, transitionKey, connection.sourceHandle, connection.targetHandle);
      syncWorkflowDraftToJson();
      if (selectedStepId.value === edge.source || selectedStepId.value === connection.source) {
        populateStepEditor(connection.source);
      }
      return;
    }

    if (data.kind === "branch" && typeof data.branchIndex === "number") {
      const oldBranches = Array.isArray(oldSourceNode?.transitions?.branches) ? oldSourceNode!.transitions.branches : [];
      const branch = oldBranches[data.branchIndex] ?? { when: {}, target: nodeRef(connection.target) };
      if (oldSourceNode && edge.source !== connection.source) {
        oldBranches.splice(data.branchIndex, 1);
        removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, `branches.${data.branchIndex}`);
      }
      newSourceNode.transitions = newSourceNode.transitions ?? {};
      newSourceNode.transitions.branches = Array.isArray(newSourceNode.transitions.branches) ? newSourceNode.transitions.branches : [];
      const branchIndex = edge.source === connection.source ? data.branchIndex : newSourceNode.transitions.branches.length;
      newSourceNode.transitions.branches[branchIndex] = { ...branch, target: nodeRef(connection.target) };
      setWorkflowEdgeHandles(workflowDraft.definition, connection.source, `branches.${branchIndex}`, connection.sourceHandle, connection.targetHandle);
      syncWorkflowDraftToJson();
      if (selectedStepId.value === edge.source || selectedStepId.value === connection.source) {
        populateStepEditor(connection.source);
      }
    }
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
    isDirty.value = true;
    return true;
  }

  function syncWorkflowDraftToJson() {
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    workflowJson.value = pretty(workflowDraft.definition);
    isDirty.value = true;
  }

  function ensureWorkflowNodes(): JsonRecord[] {
    if (!workflowDraft.definition || typeof workflowDraft.definition !== "object") workflowDraft.definition = {};
    if (!Array.isArray(workflowDraft.definition.nodes)) workflowDraft.definition.nodes = [];
    return workflowDraft.definition.nodes;
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
    stepEditor.condition_branches.push({ when_json: pretty({ value: valueRef("input", ["value"]), equals: true }), target: "" });
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
    populateStepEditor(nodeId);
    stepEditorCreating.value = creating;
    stepEditorCreatedNodeId.value = creating ? nodeId : "";
    stepEditorError.value = "";
    workflowInspectorMode.value = "step";
    stepEditorOpen.value = true;
  }

  function submitStepEditor() {
    if (applyStepEditor()) {
      stepEditorOpen.value = false;
      stepEditorCreating.value = false;
      stepEditorCreatedNodeId.value = "";
    }
  }

  function closeStepEditor() {
    if (stepEditorCreating.value && stepEditorCreatedNodeId.value) {
      const nodeId = stepEditorCreatedNodeId.value;
      workflowDraft.definition.nodes = ensureWorkflowNodes().filter((node: JsonRecord) => node.id !== nodeId);
      delete workflowTaskDrafts.value[nodeId];
      selectedStepId.value = "";
      syncWorkflowDraftToJson();
    }
    stepEditorOpen.value = false;
    stepEditorCreating.value = false;
    stepEditorCreatedNodeId.value = "";
    stepEditorError.value = "";
  }

  function duplicateSelectedStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) return;
    const nodes = ensureWorkflowNodes();
    const source = nodes.find((node: JsonRecord) => node.id === selectedStepId.value);
    if (!source) return;
    const copy = cloneJson(source);
    copy.id = uniqueWorkflowNodeId(nodes, `${source.id}_copy`);
    nodes.push(copy);
    setGraphNodePosition(copy.id, nextNodePosition(nodes.length));
    syncWorkflowDraftToJson();
    populateStepEditor(copy.id);
    openStepEditor(copy.id, true);
  }

  function setStepEditorError(message: string) {
    stepEditorError.value = message;
    app.setError(message);
  }

  function parseStepJson(label: string, text: string): { ok: true; value: any } | { ok: false } {
    const value = parseRequiredJson(text);
    if (value !== null || text.trim() === "null") return { ok: true, value };
    setStepEditorError(`${label} must be valid JSON`);
    return { ok: false };
  }

  function validateStepParameters(parameters: JsonRecord): string {
    if (stepEditor.kind !== "task" && stepEditor.kind !== "action") return "";
    const provider = useProvidersStore().providers.find((item) => item.name === stepEditor.action_name);
    const action = provider?.actions.find((item) => item.function_name === stepEditor.action_function);
    if (!action) return "Select a valid task provider action";
    for (const parameter of action.parameters ?? []) {
      if (!parameter.required) continue;
      const value = parameters[parameter.name];
      if (value === undefined || value === null || value === "") {
        return `${parameter.label || parameter.name} is required`;
      }
      if (parameter.value_type === "string_array" || parameter.value_type === "number_array") {
        if (!Array.isArray(value)) return `${parameter.label || parameter.name} must be a list`;
      } else if (parameter.value_type === "object" || parameter.value_type === "json") {
        if (typeof value !== "object") return `${parameter.label || parameter.name} must be an object`;
      } else if (parameter.value_type === "integer" || parameter.value_type === "number") {
        if (typeof value !== "number") return `${parameter.label || parameter.name} must be a number`;
      } else if (parameter.value_type === "boolean" && typeof value !== "boolean") {
        return `${parameter.label || parameter.name} must be true or false`;
      }
    }
    return "";
  }

  async function saveSelectedWorkflowBundle() {
    if (!syncWorkflowJson()) return;
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    const saved = await app.runOperation("Saving workflow", () => saveWorkflowBundle({ workflow: cloneJson(workflowDraft), tasks: [] }));
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(saved.workflow)));
    workflowJson.value = pretty(workflowDraft.definition);
    app.setStatus(`Workflow saved: ${saved.workflow.name}`);
    isDirty.value = false;
    selectedWorkflowId.value = saved.workflow.id;
    await refreshWorkflows();
  }

  async function deleteSelectedWorkflow() {
    const workflow = selectedWorkflow.value;
    if (!workflow?.id) return;
    if (!window.confirm(`Delete workflow "${workflow.name}"? This cannot be undone.`)) return;
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
      workflowJson.value = pretty(workflowDraft.definition);
      workflowRuns.value = [];
      workflowRunDetail.value = null;
      selectedWorkflowRunId.value = 0;
      isDirty.value = false;
    }
  }

  return {
    recentWorkflowRuns,
    getTransition,
    setTransition,
    workflows,
    selectedWorkflowId,
    workflowDraft,
    workflowJson,
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
    workflowTaskDrafts,
    selectedWorkflow,
    canRunWorkflow,
    canManageWorkflowTriggers,
    canRemoveSelectedStep,
    filteredWorkflows,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    workflowRunWorkflow,
    runGraphNodes,
    runGraphEdges,
    selectedNode,
    selectedNodePendingApproval,
    canStepWorkflowRun,
    workflowNodeKinds,
    directTransitionKeys,
    refreshWorkflows,
    selectWorkflow,
    addWorkflow,
    saveSelectedWorkflow: saveSelectedWorkflowBundle,
    deleteSelectedWorkflow,
    runSelectedWorkflow,
    runSelectedWorkflowDebug,
    stepSelectedWorkflowRun,
    fetchWorkflowRunsForSelected,
    fetchRecentWorkflowRuns,
    selectWorkflowRun,
    fetchWorkflowRunDetail,
    setWorkflowRunDetail,
    selectWorkflowRunNode,
    addWorkflowStep,
    addWorkflowNode,
    removeWorkflowStep,
    removeWorkflowNode,
    removeWorkflowEdgeById,
    applyStepEditor,
    populateStepEditor,
    updateSelectedWorkflowNodeDetail,
    onGraphNodeClick,
    onGraphNodeDoubleClick,
    onGraphNodeDragStop,
    onGraphNodesChange,
    onGraphConnect,
    onGraphEdgeUpdate,
    onGraphEdgesChange,
    autoArrangeWorkflowNodes,
    isDirty,
    syncWorkflowJson,
    syncWorkflowDraftToJson,
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

function nodeRefArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map(nodeRefId).filter((item): item is string => Boolean(item)) : [];
}

function branchPolicyName(value: unknown, fallback: BranchPolicyName): BranchPolicyName {
  return value === "all" || value === "any" || value === "first_success" ? value : fallback;
}

function switchCaseEditor(value: JsonRecord): SwitchCaseEditor {
  const target = nodeRefId(value.target) ?? "";
  if (value.when !== undefined) return { match_kind: "when", match_json: pretty(value.when), target };
  if (value.not_equals !== undefined) return { match_kind: "not_equals", match_json: pretty(value.not_equals), target };
  if (value.exists !== undefined) return { match_kind: "exists", match_json: pretty(Boolean(value.exists)), target };
  return { match_kind: "equals", match_json: pretty(value.equals ?? ""), target };
}

export function newWorkflowTriggerDraft(workflowId: number, kind: WorkflowTriggerKind = "cron"): WorkflowTrigger {
  return {
    id: null,
    workflow_id: workflowId,
    kind,
    enabled: true,
    configuration: defaultTriggerConfiguration(kind),
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {}
  };
}

function defaultTriggerConfiguration(kind: WorkflowTriggerKind): JsonRecord {
  if (kind === "cron") return { cron: "0 * * * *", parameters: {} };
  return {};
}

function dateTimeLocalToIso(value: string | null | undefined): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toISOString();
}

export function newWorkflowDraft(): WorkflowDefinition {
  return {
    id: null,
    name: "New Workflow",
    version: 1,
    enabled: true,
    input_schema: { type: "object", additionalProperties: true },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: nodeRef("end") } },
        { id: "end", kind: "end" },
        { id: "fail", kind: "fail" }
      ],
      ui: {
        layout: {
          nodes: {
            start: { x: 0, y: 0 },
            end: { x: 270, y: 0 },
            fail: { x: 270, y: 150 }
          }
        }
      }
    }
  };
}

function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}

function formatMaybeDate(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function normalizeNewNodeTargets(node: JsonRecord, endId: string) {
  node.transitions = node.transitions ?? {};
  for (const key of ["next", "on_success", "on_reject"]) {
    if (nodeRefId(node.transitions[key]) === "end") node.transitions[key] = nodeRef(endId);
  }
  if (Array.isArray(node.transitions.branches)) {
    for (const branch of node.transitions.branches) {
      if (nodeRefId(branch.target) === "end") branch.target = nodeRef(endId);
    }
  }
  if (nodeRefId(node.parameters?.target) === "end") node.parameters.target = nodeRef(endId);
  if (nodeRefId(node.parameters?.default) === "end") node.parameters.default = nodeRef(endId);
}

function nextNodePosition(count: number): { x: number; y: number } {
  return { x: ((count - 1) % 4) * 230, y: Math.floor((count - 1) / 4) * 130 };
}
