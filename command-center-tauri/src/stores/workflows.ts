import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import {
  createWorkflowRun,
  fetchRunArtifacts,
  fetchRunChunks,
  fetchWorkflowRun,
  fetchWorkflowRuns,
  fetchWorkflows,
  saveWorkflow
} from "../api/commandCenterApi";
import type { Edge } from "@vue-flow/core";
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, WorkflowDefinition, WorkflowNodeKind, WorkflowRunDetail } from "../types/models";
import { pretty } from "../utils/format";
import { cloneJson, parseObject, parseRequiredObject } from "../utils/json";
import {
  addDirectTransition,
  buildGraphEdges,
  buildGraphNodes,
  createWorkflowNode,
  directTransitionKeys,
  nodeRef,
  nodeRefId,
  normalizeWorkflowDefinition,
  removeConditionBranch,
  removeEditableEdge,
  setConditionBranch,
  validateWorkflowReferenceSyntax,
  valueRef,
  workflowNodeKinds
} from "../utils/workflows";
import { useAppStore } from "./app";
import { useResourcesStore } from "./resources";
import { useTasksStore } from "./tasks";

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<number | null>(null);
  const workflowDraft = reactive<WorkflowDefinition>(newWorkflowDraft());
  const workflowJson = ref("{}");
  const workflowConcurrency = ref(1);
  const workflowSettingsOpen = ref(false);
  const workflowEditorMode = ref<"graph" | "json">("graph");
  const workflowInspectorMode = ref<"step" | "runs" | "detail">("step");
  const workflowRuns = ref<RunSummary[]>([]);
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
    task_id: 1,
    approval_type: "generic",
    approval_prompt: "Approval required",
    condition_fallback: "",
    condition_branches: [] as Array<{ when_json: string; target: string }>,
    wait_json: "{}",
    max_attempts: 1,
    timeout_seconds: 0,
    parameters_json: "{}",
    transitions_json: "{}"
  });

  const isDirty = ref(false);

  const app = useAppStore();
  const selectedWorkflow = computed(() => workflows.value.find((workflow) => workflow.id === selectedWorkflowId.value) ?? null);
  const canRunWorkflow = computed(() => Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id));
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
  const graphNodes = computed(() => buildGraphNodes(workflowDraft, workflowRunDetail.value, useTasksStore().tasks));
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
    if (isDirty.value && selectedWorkflowId.value !== workflow.id) {
      // If we're changing workflows while dirty, we might want to warn,
      // but if refreshWorkflows calls this, we already check !isDirty.
    }
    selectedWorkflowId.value = workflow.id;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflow)));
    
    workflowConcurrency.value = Number(workflowDraft.definition?.concurrency ?? 1);
    workflowJson.value = pretty(workflowDraft.definition ?? { nodes: [] });
    selectedStepId.value = "";
    workflowRunDetail.value = null;
    isDirty.value = false;
    if (workflow.id) return fetchWorkflowRunsForSelected(workflow.id);
    return Promise.resolve();
  }

  function addWorkflow() {
    const workflow = newWorkflowDraft();
    workflows.value.push(workflow);
    selectWorkflow(workflow);
  }

  async function saveSelectedWorkflow() {
    if (!syncWorkflowJson()) return;
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    Object.assign(workflowDraft, normalizeWorkflowDefinition(cloneJson(workflowDraft)));
    workflowJson.value = pretty(workflowDraft.definition);
    const saved = await app.runOperation("Saving workflow", () => saveWorkflow(workflowDraft));
    app.setStatus(`Workflow saved: ${saved.name}`);
    isDirty.value = false;
    selectedWorkflowId.value = saved.id;
    await refreshWorkflows();
  }

  async function runSelectedWorkflow() {
    const workflow = selectedWorkflow.value;
    if (!workflow?.id || !workflow.enabled) return app.setError(workflow ? "Workflow is disabled" : "No workflow selected");
    const response = await app.runOperation(`Running workflow ${workflow.name}`, () => createWorkflowRun(workflow.id!));
    selectedWorkflowRunId.value = response.id;
    app.setStatus(`Workflow run queued: ${response.id}`);
    await fetchWorkflowRunDetail(response.id);
    await fetchWorkflowRunsForSelected(workflow.id);
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

  async function fetchWorkflowRunDetail(workflowRunId: number) {
    workflowRunDetail.value = await app.runOperation("Loading workflow run", () => fetchWorkflowRun(workflowRunId)).catch(() => null);
    workflowNodeDetailExtra.value = "";
    if (workflowRunDetail.value) {
      workflowInspectorMode.value = "detail";

      const resources = useResourcesStore();
      const hasWaiting = workflowRunDetail.value.nodes.some(n => 
        n.status === "waiting" || n.status === "approval_required" || n.status === "pending"
      );
      if (hasWaiting) {
        // Run in background to not block the detail view
        resources.refreshResources();
      }
    }
  }

  function addWorkflowStep() {
    addWorkflowNode("task");
  }

  function addWorkflowNode(kind: WorkflowNodeKind) {
    if (!syncWorkflowJson()) return;
    const nodes = ensureWorkflowNodes();
    const tasks = useTasksStore();
    const newNode = createWorkflowNode(kind, nodes, tasks.tasks[0]?.id ?? 1);
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
  }

  function removeWorkflowStep() {
    if (!selectedStepId.value || !canRemoveSelectedStep.value) return;
    workflowDraft.definition.nodes = ensureWorkflowNodes().filter((node: JsonRecord) => node.id !== selectedStepId.value);
    selectedStepId.value = "";
    syncWorkflowDraftToJson();
  }

  function applyStepEditor() {
    if (!selectedStepId.value) return;
    const nodes = ensureWorkflowNodes();
    const index = nodes.findIndex((node: JsonRecord) => node.id === selectedStepId.value);
    if (index < 0) return;
    const parameters = parseRequiredObject(stepEditor.parameters_json);
    const transitions = parseRequiredObject(stepEditor.transitions_json);
    if (!parameters || !transitions) return app.setError(parameters ? "Node transitions must be a JSON object" : "Step parameters must be a JSON object");
    const next = { ...nodes[index] };
    next.id = stepEditor.id.trim();
    next.kind = stepEditor.kind;
    if (next.kind === "task") next.task_id = stepEditor.task_id;
    else delete next.task_id;
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
        if (!when) return app.setError(`Condition branch ${branchIndex + 1} must be a JSON object`);
        if (!branch.target) return app.setError(`Condition branch ${branchIndex + 1} needs a target`);
        setConditionBranch(next, branchIndex, when, branch.target);
      }
      if (stepEditor.condition_fallback) next.transitions.next = nodeRef(stepEditor.condition_fallback);
      else delete next.transitions.next;
    }
    if (next.kind === "wait") {
      const wait = parseRequiredObject(stepEditor.wait_json);
      if (!wait) return app.setError("Wait settings must be a JSON object");
      next.wait = wait;
    }
    nodes[index] = next;
    if (selectedStepId.value !== next.id) renameLayoutNode(selectedStepId.value, next.id);
    selectedStepId.value = next.id;
    syncWorkflowDraftToJson();
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
    const nodes = ensureWorkflowNodes();
    const sourceNode = nodes.find((n: JsonRecord) => n.id === source);
    if (!sourceNode) return;
    addDirectTransition(sourceNode, target, sourceHandle);
    syncWorkflowDraftToJson();
    if (selectedStepId.value === source) {
      populateStepEditor(source);
    }
  }

  function onGraphEdgesChange(changes: any[]) {
    let changed = false;
    for (const change of changes) {
      if (change.type === "remove") {
        const edge = graphEdges.value.find((item: Edge) => item.id === change.id);
        if (edge) {
          const sourceNode = ensureWorkflowNodes().find((n: JsonRecord) => n.id === edge.source);
          if (sourceNode && removeEditableEdge(sourceNode, edge)) changed = true;
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
    workflowRuns,
    selectedWorkflowRunId,
    workflowRunDetail,
    workflowNodeDetailExtra,
    selectedStepId,
    selectedWorkflowNodeTaskRunId,
    stepEditor,
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
    workflowNodeKinds,
    directTransitionKeys,
    refreshWorkflows,
    selectWorkflow,
    addWorkflow,
    saveSelectedWorkflow,
    runSelectedWorkflow,
    fetchWorkflowRunsForSelected,
    selectWorkflowRun,
    fetchWorkflowRunDetail,
    addWorkflowStep,
    addWorkflowNode,
    removeWorkflowStep,
    applyStepEditor,
    populateStepEditor,
    updateSelectedWorkflowNodeDetail,
    onGraphNodeClick,
    onGraphNodeDragStop,
    onGraphNodesChange,
    onGraphConnect,
    onGraphEdgesChange,
    isDirty,
    syncWorkflowJson,
    syncWorkflowDraftToJson,
    ensureWorkflowNodes,
    addConditionBranchEditor,
    removeConditionBranchEditor,
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
