<template src="./workflow-run-detail.template.html"></template>

<script setup lang="ts">
/* eslint-disable @typescript-eslint/no-unused-vars -- external Vue templates are not visible to ESLint. */
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useProvidersStore } from "../../../ui/adapters/pinia/providers";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import { usePipelineRunsStore } from "../../../ui/adapters/pinia/pipeline-runs";
import { useOrchestrationsStore } from "../../../ui/adapters/pinia/orchestrations";
import Icon from "../shared/Icon.vue";
import StatusBadge from "../shared/StatusBadge.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import RunTimeline from "../shared/RunTimeline.vue";
import RunGantt from "../shared/RunGantt.vue";
import RunNodeActions, { type RunNodeActionType } from "../shared/RunNodeActions.vue";
import CursorRail from "./CursorRail.vue";
import DebugControlBar from "./DebugControlBar.vue";
import RunControlBar from "./RunControlBar.vue";
import JsonDiff from "./JsonDiff.vue";
import WatchExpressions from "./WatchExpressions.vue";
import { formatDate, formatErrorMessage, pretty } from "../../../core/utils/format";
import { computed, nextTick, ref, watch } from "vue";
import type {
  ActionResultMetadata,
  DebugFrame,
  ExternalOperation,
  OrchestrationBinding,
  WorkflowNodeRun,
  WorkspaceLease,
} from "../../../core/domain/models";
import { coerceDebugFrame } from "../../../core/domain/models/workflow-state";
import {
  asArray,
  isRecord,
  workflowNodeActionConfig,
  workflowNodeResultMetadata,
} from "../../../core/workflow";
import { formatResultValue, formatRunDuration, shortId } from "./run-detail-format";
import { useWorkflowTransitionStats } from "./useWorkflowTransitionStats";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const app = useAppStore();
const pipelineRuns = usePipelineRunsStore();
const orchestrations = useOrchestrationsStore();

const parentPipelineRunId = computed(() => workflows.workflowRunDetail?.run.pipeline_run_id ?? null);
const parentPipelineRun = computed(() =>
  pipelineRuns.detail?.run.id === parentPipelineRunId.value ? pipelineRuns.detail.run : null,
);
const managedBindingId = computed(() => parentPipelineRun.value?.orchestration_binding_id ?? null);
const managedBinding = computed<OrchestrationBinding | null>(() => {
  const selected = orchestrations.selected;
  return selected?.id === managedBindingId.value ? selected : null;
});
const managedWorkspaces = computed<WorkspaceLease[]>(() =>
  orchestrations.selectedId === managedBindingId.value
    ? orchestrations.workspaces
    : [],
);
const managedOperations = computed<ExternalOperation[]>(() => {
  const runId = workflows.workflowRunDetail?.run.id;
  const operations = orchestrations.operations as unknown as ExternalOperation[];

  return orchestrations.selectedId === managedBindingId.value
    ? operations.filter((operation) => operation.workflow_run_id === runId)
    : [];
});

async function openOrchestration(): Promise<void> {
  if (!managedBindingId.value) {return;}
  await orchestrations.select(managedBindingId.value);
  app.activeTab = "Orchestrations";
}

watch(parentPipelineRunId, (pipelineRunId) => {
  if (pipelineRunId) {
    void pipelineRuns.selectRun(pipelineRunId);
  }
}, { immediate: true });

watch(managedBindingId, (bindingId) => {
  if (bindingId) {
    void orchestrations.select(bindingId);
  }
}, { immediate: true });

const renaming = ref(false);
const renameDraft = ref("");
const renameInput = ref<HTMLInputElement | null>(null);

const runHeadingLabel = computed(() => {
  const run = workflows.workflowRunDetail?.run;

  if (!run) {
    return "Workflow Run";
  }

  const trimmed = run.name?.trim();
  return trimmed ? `${trimmed} (#${run.id})` : `Workflow Run #${run.id}`;
});

async function startRename() {
  const run = workflows.workflowRunDetail?.run;

  if (!run) {
    return;
  }

  renameDraft.value = run.name?.trim() ?? "";
  renaming.value = true;
  await nextTick();
  renameInput.value?.focus();
  renameInput.value?.select();
}

function cancelRename() {
  renaming.value = false;
  renameDraft.value = "";
}

async function commitRename() {
  if (!renaming.value) {
    return;
  }

  const run = workflows.workflowRunDetail?.run;

  if (!run) {
    renaming.value = false;
    return;
  }

  const next = renameDraft.value.trim();
  const previous = run.name?.trim() ?? "";
  renaming.value = false;

  if (next === previous) {
    return;
  }

  await workflows.renameSelectedWorkflowRun(run.id, next.length === 0 ? null : next);
}

async function deleteRun() {
  const run = workflows.workflowRunDetail?.run;

  if (!run || !window.confirm("Permanently delete this workflow run and all execution history?")) {
    return;
  }

  await workflows.deleteSelectedWorkflowRun();
}

// quick actions emitted by RunNodeActions in the timeline (feature 7).
async function onNodeAction(payload: { type: RunNodeActionType; node: WorkflowNodeRun }) {
  const run = workflows.workflowRunDetail?.run;

  if (!run) {
    return;
  }

  if (payload.type === "replay-run") {
    await workflows.replaySelectedWorkflowRun(run.id);
  } else if (payload.type === "replay-from") {
    await workflows.replaySelectedWorkflowRun(run.id, payload.node.node_id);
  } else if (payload.type === "open-editor") {
    await openStepInEditor(payload.node.node_id);
  } else if (payload.type === "open-provider") {
    openProviderForNode(payload.node.node_id);
  }
}

// look the run's node up in its workflow definition.
function definitionNode(nodeId: string) {
  return (
    asArray(workflows.workflowRunWorkflow?.definition.nodes)
      .filter(isRecord)
      .find((node) => node.id === nodeId) ?? null
  );
}

// open the step in the workflow editor, preferring the live workflow over the run snapshot.
async function openStepInEditor(nodeId: string) {
  const workflowId = workflows.workflowRunWorkflow?.id;
  const workflow =
    workflows.workflows.find((item) => item.id === workflowId) ?? workflows.workflowRunWorkflow;

  if (!workflow) {
    return;
  }

  await workflows.selectWorkflow(workflow);
  app.activeTab = "Workflows";
  workflows.openStepEditor(nodeId);
}

// focus this node's provider/action in the providers view.
function openProviderForNode(nodeId: string) {
  const node = definitionNode(nodeId);

  if (!node) {
    return;
  }

  const config = workflowNodeActionConfig(node);

  if (!config.provider) {
    return;
  }

  providersStore.focusProviderAction(config.provider, config.action);
  app.activeTab = "Providers";
}

const selectedNodeOutput = computed<Record<string, unknown> | null>(() => {
  const node = workflows.workflowRunDetail?.nodes.find(
    (item) => item.node_id === workflows.selectedWorkflowRunNodeId,
  );
  const output = node?.output_json;

  if (output && typeof output === "object" && !Array.isArray(output)) {
    return output;
  }

  return null;
});

const debugState = computed<DebugFrame | null>(() => {
  return coerceDebugFrame(workflows.workflowRunDetail?.execution_state?.debug) ?? null;
});

const inputJsonText = computed(() => pretty(debugState.value?.input_json ?? {}));
const lastOutputJsonText = computed(() => pretty(debugState.value?.last_output_json ?? null));
const contextJsonText = computed(() => pretty(debugState.value?.context_json ?? {}));

const TERMINAL_STATUSES = new Set(["succeeded", "failed", "canceled", "timed_out"]);
const isTerminalRun = computed(() => {
  const status = workflows.workflowRunDetail?.run.status;
  return Boolean(status && TERMINAL_STATUSES.has(status));
});

// only show the proportional timeline once at least one node has real start/finish timing.
const hasNodeTiming = computed(() =>
  (workflows.workflowRunDetail?.nodes ?? []).some(
    (node) => node.started_at != null || node.finished_at != null,
  ),
);

const nodeCounts = computed(() => {
  const counts = { succeeded: 0, failed: 0, canceled: 0 };

  for (const node of workflows.workflowRunDetail?.nodes ?? []) {
    if (node.status === "succeeded") {
      counts.succeeded += 1;
    } else if (node.status === "failed" || node.status === "timed_out") {
      counts.failed += 1;
    } else if (node.status === "canceled") {
      counts.canceled += 1;
    }
  }

  return counts;
});

const runDurationText = computed(() => {
  const run = workflows.workflowRunDetail?.run;
  return formatRunDuration(run?.started_at, run?.finished_at);
});

const selectedNodeResultText = computed(() => {
  const node = workflows.workflowRunDetail?.nodes.find(
    (item) => item.node_id === workflows.selectedWorkflowRunNodeId,
  );
  return pretty(node?.output_json ?? {});
});

const resultFields = computed<ActionResultMetadata[]>(() => {
  const nodeId = workflows.selectedWorkflowRunNodeId;

  if (!nodeId) {
    return [];
  }

  const definition =
    workflows.workflowRunWorkflow?.definition ?? workflows.workflowDraft.definition;
  const defNode = asArray(definition.nodes)
    .filter(isRecord)
    .find((n) => n.id === nodeId);

  if (!defNode) {
    return [];
  }

  return workflowNodeResultMetadata(defNode, providersStore.providers);
});

const hasExtraFields = computed(() => {
  if (!selectedNodeOutput.value) {
    return false;
  }

  const knownNames = new Set(resultFields.value.map((f) => f.name));
  return Object.keys(selectedNodeOutput.value).some((k) => !knownNames.has(k));
});

// a flat, creation-ordered view of the run's node runs for debugging. each row carries its own guid
// and a pointer to the previously created step, forming a linked chain that is easier to follow than
// the nested `steps.<node>` output tree.
const flatSteps = computed<WorkflowNodeRun[]>(() => {
  const nodes = [...(workflows.workflowRunDetail?.nodes ?? [])];
  return nodes.sort((a, b) => {
    const at = a.created_at ? Date.parse(a.created_at) : 0;
    const bt = b.created_at ? Date.parse(b.created_at) : 0;

    if (at !== bt) {
      return at - bt;
    }

    return a.id.localeCompare(b.id);
  });
});

function selectByRunId(runId: string) {
  const node = workflows.workflowRunDetail?.nodes.find((item) => item.id === runId);

  if (node) {
    workflows.selectWorkflowRunNode(node.node_id);
  }
}

const { runTransitions, sortedNodeStats, statPercent } = useWorkflowTransitionStats(workflows);
</script>
