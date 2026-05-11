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
import type { JsonRecord, RunArtifact, RunChunk, RunSummary, WorkflowDefinition, WorkflowRunDetail } from "../types/models";
import { pretty } from "../utils/format";
import { cloneJson, parseObject, parseRequiredObject } from "../utils/json";
import { buildGraphEdges, buildGraphNodes } from "../utils/workflows";
import { useAppStore } from "./app";
import { useResourcesStore } from "./resources";
import { useTasksStore } from "./tasks";

export const useWorkflowsStore = defineStore("workflows", () => {
  const workflows = ref<WorkflowDefinition[]>([]);
  const selectedWorkflowId = ref<number | null>(null);
  const workflowDraft = reactive<WorkflowDefinition>(newWorkflowDraft());
  const workflowJson = ref("{}");
  const workflowConcurrency = ref(1);
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
    task_id: 1,
    max_attempts: 1,
    timeout_seconds: 0,
    parameters_json: "{}",
    transitions_json: "{}"
  });

  const isDirty = ref(false);

  const app = useAppStore();
  const selectedWorkflow = computed(() => workflows.value.find((workflow) => workflow.id === selectedWorkflowId.value) ?? null);
  const canRunWorkflow = computed(() => Boolean(selectedWorkflow.value?.enabled && selectedWorkflow.value.id));
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
      .map((key) => `${key}:${transitions[key]}`)
      .join(",");
  });
  const graphNodes = computed(() => buildGraphNodes(workflowDraft, workflowRunDetail.value));
  const graphEdges = computed(() => buildGraphEdges(workflowDraft));

  async function refreshWorkflows() {
    workflows.value = await app.runOperation("Refreshing workflows", () => fetchWorkflows()).catch(() => []);
    if (!selectedWorkflowId.value && workflows.value.length > 0) selectedWorkflowId.value = workflows.value[0].id;
    const workflow = workflows.value.find((item) => item.id === selectedWorkflowId.value) ?? workflows.value[0];
    if (workflow && !isDirty.value) await selectWorkflow(workflow);
  }

  function getTransition(key: string): string {
    const transitions = parseObject(stepEditor.transitions_json, {});
    return transitions[key] ?? "";
  }

  function setTransition(key: string, value: string) {
    const transitions = parseObject(stepEditor.transitions_json, {});
    if (value) {
      transitions[key] = value;
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
    Object.assign(workflowDraft, cloneJson(workflow));
    if (!workflowDraft.definition) workflowDraft.definition = { nodes: [] };
    if (!workflowDraft.definition.nodes) workflowDraft.definition.nodes = [];
    
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
    syncWorkflowJson();
    const nodes = ensureWorkflowNodes();
    const tasks = useTasksStore();
    const id = `node_${nodes.length + 1}`;
    nodes.push({
      id,
      kind: "task",
      task_id: tasks.tasks[0]?.id ?? 1,
      parameters: {},
      retry: { max_attempts: 1 },
      transitions: {}
    });
    if (!workflowDraft.definition.start) workflowDraft.definition.start = id;
    syncWorkflowDraftToJson();
    populateStepEditor(id);
  }

  function removeWorkflowStep() {
    if (!selectedStepId.value) return;
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
    next.kind = next.kind ?? "task";
    if (next.kind === "task") next.task_id = stepEditor.task_id;
    next.retry = { max_attempts: stepEditor.max_attempts };
    if (stepEditor.timeout_seconds > 0) next.timeout_seconds = stepEditor.timeout_seconds;
    else delete next.timeout_seconds;
    next.parameters = parameters;
    next.transitions = transitions;
    nodes[index] = next;
    selectedStepId.value = next.id;
    syncWorkflowDraftToJson();
  }

  function populateStepEditor(nodeId: string) {
    const node = ensureWorkflowNodes().find((item: JsonRecord) => item.id === nodeId);
    if (!node) return;
    selectedStepId.value = nodeId;
    stepEditor.id = nodeId;
    stepEditor.task_id = Number(node.task_id ?? 1);
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
    const definition = workflowDraft.definition;
    definition.ui = definition.ui ?? {};
    definition.ui.layout = definition.ui.layout ?? {};
    definition.ui.layout.nodes = definition.ui.layout.nodes ?? {};
    definition.ui.layout.nodes[node.id] = { x: node.position.x, y: node.position.y };
    syncWorkflowDraftToJson();
  }

  function onGraphConnect(connection: any) {
    const { source, target } = connection;
    if (!source || !target) return;
    const nodes = ensureWorkflowNodes();
    const sourceNode = nodes.find((n: JsonRecord) => n.id === source);
    if (!sourceNode) return;
    sourceNode.transitions = sourceNode.transitions ?? {};
    // Default to 'next' if it's empty, otherwise don't overwrite existing unless they are different?
    // Actually, Vue Flow 'connect' usually means creating a new edge.
    // We'll use 'next' as the default transition type.
    if (!sourceNode.transitions.next) {
      sourceNode.transitions.next = target;
    } else if (!sourceNode.transitions.on_success) {
      sourceNode.transitions.on_success = target;
    } else {
      // If next and on_success are taken, maybe use a generic transition if it were supported,
      // but here we stick to the known keys.
      sourceNode.transitions.next = target;
    }
    syncWorkflowDraftToJson();
    if (selectedStepId.value === source) {
      populateStepEditor(source);
    }
  }

  function onGraphEdgesChange(changes: any[]) {
    let changed = false;
    for (const change of changes) {
      if (change.type === "remove") {
        const edgeId = change.id;
        // Edge ID is constructed as `${source}-${key}-${target}` in utils/workflows.ts
        const parts = edgeId.split("-");
        if (parts.length >= 3) {
          const source = parts[0];
          const key = parts[1];
          const target = parts[2];
          const nodes = ensureWorkflowNodes();
          const sourceNode = nodes.find((n: JsonRecord) => n.id === source);
          if (sourceNode && sourceNode.transitions && sourceNode.transitions[key] === target) {
            delete sourceNode.transitions[key];
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

  function syncWorkflowJson(): boolean {
    const parsed = parseRequiredObject(workflowJson.value);
    if (!parsed) {
      app.setError("Workflow definition must be a JSON object");
      return false;
    }
    workflowDraft.definition = parsed;
    workflowDraft.definition.concurrency = workflowConcurrency.value;
    isDirty.value = true;
    return true;
  }

  function syncWorkflowDraftToJson() {
    workflowDraft.definition.concurrency = workflowConcurrency.value;
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

  return {
    recentWorkflowRuns,
    getTransition,
    setTransition,
    workflows,
    selectedWorkflowId,
    workflowDraft,
    workflowJson,
    workflowConcurrency,
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
    filteredWorkflows,
    workflowRunDetailText,
    stepNeeds,
    graphNodes,
    graphEdges,
    refreshWorkflows,
    selectWorkflow,
    addWorkflow,
    saveSelectedWorkflow,
    runSelectedWorkflow,
    fetchWorkflowRunsForSelected,
    selectWorkflowRun,
    fetchWorkflowRunDetail,
    addWorkflowStep,
    removeWorkflowStep,
    applyStepEditor,
    populateStepEditor,
    updateSelectedWorkflowNodeDetail,
    onGraphNodeClick,
    onGraphNodeDragStop,
    onGraphConnect,
    onGraphEdgesChange,
    isDirty,
    syncWorkflowJson,
    syncWorkflowDraftToJson,
    ensureWorkflowNodes,
    moveWorkflowSelection
  };
});

export function newWorkflowDraft(): WorkflowDefinition {
  return {
    id: null,
    name: "New Workflow",
    version: 1,
    enabled: true,
    input_schema: { type: "object", additionalProperties: true },
    definition: { start: "node_1", nodes: [] }
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
