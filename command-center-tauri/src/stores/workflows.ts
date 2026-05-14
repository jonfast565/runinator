import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  createWorkflowRun,
  fetchRunArtifacts,
  fetchRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  fetchWorkflows,
  saveWorkflowBundle,
  stepWorkflowRun
} from "../api/commandCenterApi";
import type { Edge } from "@vue-flow/core";
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, ScheduledTask, WorkflowDefinition, WorkflowNodeKind, WorkflowRunDetail } from "../types/models";
import { pretty } from "../utils/format";
import { cloneJson, parseObject, parseRequiredObject } from "../utils/json";
import {
  addDirectTransition,
  autoArrangeWorkflowLayout,
  buildGraphEdges,
  buildGraphNodes,
  copyWorkflowTaskDraft,
  createWorkflowTaskDraft,
  createWorkflowNode,
  directTransitionKeys,
  isSameConnectionPointLoop,
  nodeRef,
  nodeRefId,
  normalizeWorkflowDefinition,
  removeConditionBranch,
  removeEditableEdge,
  removeWorkflowEdgeHandles,
  setConditionBranch,
  setWorkflowEdgeHandles,
  stampWorkflowTaskMetadata,
  uniqueWorkflowNodeId,
  validateWorkflowReferenceSyntax,
  valueRef,
  workflowNodeKinds
} from "../utils/workflows";
import { useAppStore } from "./app";
import { useProvidersStore } from "./providers";
import { useResourcesStore } from "./resources";

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<number | null>(null);
  const workflowDraft = reactive<WorkflowDefinition>(newWorkflowDraft());
  const workflowJson = ref("{}");
  const workflowConcurrency = ref(1);
  const workflowSettingsOpen = ref(false);
  const workflowEditorMode = ref<"graph" | "json">("graph");
  const workflowInspectorMode = ref<"step" | "runs" | "detail">("step");
  const stepEditorOpen = ref(false);
  const stepEditorCreating = ref(false);
  const stepEditorCreatedNodeId = ref("");
  const stepEditorError = ref("");
  const workflowRuns = ref<RunSummary[]>([]);
  const workflowLayoutVersion = ref(0);
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
    // Only return the first instance of each run ID to avoid duplicates if backend returns them separately (though unlikely for workflow runs)
    // Actually, task runs have multiple for same task, but workflow runs should be unique.
    // If user meant "concurrent runs", they might share some ID? No, each run has unique ID.
    // The issue says "completed workflows are not divided up by runs (say I ran the workflow two times concurrently)".
    // This implies the list might be confusing if multiple runs exist.
    
    const query = app.normalizedSearch;
    let list = workflowRuns.value;
    if (query) {
      list = list.filter(r => 
        String(r.id).includes(query) || 
        (r.status && r.status.toLowerCase().includes(query))
      );
    }
    return list.slice(0, 50); // Limit to 50 recent runs
  });
  const selectedWorkflowRunId = ref(0);
  const workflowRunDetail = ref<WorkflowRunDetail | null>(null);
  const workflowNodeDetailExtra = ref("");
  const selectedStepId = ref("");
  const selectedWorkflowNodeTaskRunId = ref(0);
  const stepEditor = reactive({
    id: "",
    kind: "task" as WorkflowNodeKind,
    approval_type: "generic",
    approval_prompt: "Approval required",
    condition_fallback: "",
    condition_branches: [] as Array<{ when_json: string; target: string }>,
    wait_json: "{}",
    max_attempts: 1,
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
  const canStepWorkflowRun = computed(() => workflowRunDetail.value?.run.status === "debug_paused");
  const canRemoveSelectedStep = computed(() => {
    const node = workflowDraft.definition?.nodes?.find((item: JsonRecord) => item.id === selectedStepId.value);
    return Boolean(node && node.kind !== "start" && node.kind !== "end");
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
      lines.push(`${step.node_id}: ${step.status}, attempt ${step.attempt}, task run ${step.task_run_id ?? "-"}${step.message ? `, ${step.message}` : ""}`);
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
  const graphNodes = computed(() => buildGraphNodes(workflowDraft, workflowRunDetail.value, [...useTasksStore().tasks, ...Object.values(workflowTaskDrafts.value)]));
  const graphEdges = computed(() => buildGraphEdges(workflowDraft));
  const selectedNode = computed(() => ensureWorkflowNodes().find((item: JsonRecord) => item.id === selectedStepId.value) ?? null);
  const selectedNodePendingApproval = computed(() => {
    const detail = workflowRunDetail.value;
    if (!detail || !selectedStepId.value) return null;
    return detail.nodes.filter((node) => node.node_id === selectedStepId.value && ["waiting", "approval_required", "pending"].includes(String(node.status))).at(-1) ?? null;
  });

  async function refreshWorkflows() {
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
      workflowRunDetail.value = null;
      workflowTaskDrafts.value = {};
      stepEditorOpen.value = false;
    }
    isDirty.value = false;
    if (workflow.id) return fetchWorkflowRunsForSelected(workflow.id);
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
    await fetchWorkflowRunsForSelected(workflow.id);
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
    workflowRuns.value = await app.runOperation("Loading workflow runs", () => fetchWorkflowRuns(workflowId)).catch(() => []);
    if (!workflowRuns.value.some((run) => run.id === selectedWorkflowRunId.value)) {
      selectedWorkflowRunId.value = workflowRuns.value[0]?.id ?? 0;
    }
  }

  async function selectWorkflowRun(run: RunSummary) {
    selectedWorkflowRunId.value = run.id;
    return fetchWorkflowRunDetail(run.id);
  }

  async function fetchWorkflowRunDetail(workflowRunId: number, silent = false) {
    const detail = silent
      ? await fetchWorkflowRun(workflowRunId).catch(() => null)
      : await app.runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId)).catch(() => null);
    applyWorkflowRunDetail(detail);
  }

  function setWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    applyWorkflowRunDetail(detail);
  }

  function applyWorkflowRunDetail(detail: WorkflowRunDetail | null) {
    workflowRunDetail.value = detail;
    workflowNodeDetailExtra.value = "";
    if (detail) {
      workflowInspectorMode.value = "detail";
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
    const tasks = useTasksStore();
    const taskId = kind === "task" ? nextWorkflowTaskId() : tasks.tasks[0]?.id ?? 1;
    const newNode = createWorkflowNode(kind, nodes, taskId);
    let taskDraft: ScheduledTask | null = null;
    if (kind === "task") {
      taskDraft = createWorkflowTaskDraft(newNode.id, taskId);
      taskDraft = stampWorkflowTaskMetadata(taskDraft, newNode.id, workflowDraft.id);
      const response = await app.runOperation("Creating workflow task", () => saveTask(taskDraft!, true));
      if (response.success === false) {
        app.setError(response.message || "Failed to create workflow task");
        return;
      }
      tasks.tasks.push(cloneJson(taskDraft));
      workflowTaskDrafts.value[newNode.id] = taskDraft;
    }
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
    const nodeId = selectedStepId.value;
    workflowDraft.definition.nodes = ensureWorkflowNodes().filter((item: JsonRecord) => item.id !== nodeId);
    selectedStepId.value = "";
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
    if (next.kind === "task") {
      next.action_name = stepEditor.action_name;
      next.action_function = stepEditor.action_function;
    } else {
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
      next.wait = wait;
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
    stepEditor.wait_json = pretty(node.wait ?? {});
    stepEditor.max_attempts = Number(node.retry?.max_attempts ?? 1);
    stepEditor.timeout_seconds = Number(node.timeout_seconds ?? 0);
    stepEditor.action_name = node.action_name || "";
    stepEditor.action_function = node.action_function || "";
    stepEditor.parameters_json = pretty(node.parameters ?? {});
    stepEditor.transitions_json = pretty(node.transitions ?? {});
    // If we're in detail mode, stay there, otherwise switch to step
    if (workflowInspectorMode.value !== "detail") {
      workflowInspectorMode.value = "step";
    }
    updateSelectedWorkflowNodeDetail();
  }

  async function updateSelectedWorkflowNodeDetail() {
    selectedWorkflowNodeTaskRunId.value = 0;
    workflowNodeDetailExtra.value = "";
    const step = workflowRunDetail.value?.nodes.find((node) => node.node_id === selectedStepId.value && node.task_run_id);
    if (!step?.task_run_id) return;
    selectedWorkflowNodeTaskRunId.value = step.task_run_id;
    const [nodeChunks, nodeArtifacts] = await Promise.all([
      app.runOperation("Loading node chunks", () => fetchRunChunks(step.task_run_id!)).catch(() => [] as RunChunk[]),
      app.runOperation("Loading node artifacts", () => fetchRunArtifacts(step.task_run_id!)).catch(() => [] as RunArtifact[])
    ]);
    workflowNodeDetailExtra.value = [
      "",
      `Task run ${step.task_run_id} chunks`,
      ...nodeChunks.map((chunk) => `[${chunk.stream}] ${chunk.content}`),
      "",
      `Task run ${step.task_run_id} artifacts`,
      ...nodeArtifacts.map((artifact) => `${artifact.name} (${artifact.size_bytes} bytes) ${artifact.uri}`)
    ].join("\n");
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
          if (sourceNode && removeEditableEdge(sourceNode, edge)) {
            const data = edge.data as any;
            if (data?.transitionKey) removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, data.transitionKey);
            if (typeof data?.branchIndex === "number") removeWorkflowEdgeHandles(workflowDraft.definition, edge.source, `branches.${data.branchIndex}`);
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

  function autoArrangeWorkflowNodes() {
    if (!syncWorkflowJson()) return;
    const positions = autoArrangeWorkflowLayout(workflowDraft.definition);
    for (const [nodeId, position] of Object.entries(positions)) setGraphNodePosition(nodeId, position);
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

  function openWorkflowSettings() {
    workflowSettingsOpen.value = true;
  }

  function closeWorkflowSettings() {
    workflowSettingsOpen.value = false;
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
      const task = workflowTaskDrafts.value[nodeId];
      if (task?.id && isWorkflowOwnedTask(task)) {
        deleteTask(task.id).then(() => useTasksStore().refreshTasks()).catch(() => {});
      }
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

  function validateStepParameters(parameters: JsonRecord): string {
    if (stepEditor.kind !== "task") return "";
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
    workflowEditorMode,
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
    selectedWorkflowNodeTaskRunId,
    stepEditor,
    workflowTaskDrafts,
    selectedWorkflow,
    canRunWorkflow,
    canRemoveSelectedStep,
    filteredWorkflows,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    selectedNode,
    selectedNodePendingApproval,
    canStepWorkflowRun,
    workflowNodeKinds,
    directTransitionKeys,
    refreshWorkflows,
    selectWorkflow,
    addWorkflow,
    saveSelectedWorkflow: saveSelectedWorkflowBundle,
    runSelectedWorkflow,
    runSelectedWorkflowDebug,
    stepSelectedWorkflowRun,
    fetchWorkflowRunsForSelected,
    selectWorkflowRun,
    fetchWorkflowRunDetail,
    setWorkflowRunDetail,
    addWorkflowStep,
    addWorkflowNode,
    removeWorkflowStep,
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
    openStepEditor,
    closeStepEditor,
    submitStepEditor,
    duplicateSelectedStep,
    moveWorkflowSelection,
    openWorkflowSettings,
    closeWorkflowSettings,
    markWorkflowDirty
  };
});

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
        { id: "end", kind: "end" }
      ],
      ui: {
        layout: {
          nodes: {
            start: { x: 0, y: 0 },
            end: { x: 0, y: 120 }
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
