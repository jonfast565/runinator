<template>
  <div class="inspector-section">
    <div class="detail-header">
      <h2 class="run-detail-heading">
        <template v-if="!renaming">
          <span>{{ runHeadingLabel }}</span>
          <button
            v-if="workflows.workflowRunDetail"
            class="btn btn-icon btn-ghost btn-sm"
            title="Rename run"
            @click="startRename"
          >
            <Icon name="edit" :size="14" />
          </button>
        </template>
        <template v-else>
          <input
            ref="renameInput"
            v-model="renameDraft"
            class="rename-input"
            placeholder="Run name"
            @keydown.enter.prevent="commitRename"
            @keydown.escape.prevent="cancelRename"
            @blur="commitRename"
          />
        </template>
      </h2>
      <StatusBadge :status="workflows.workflowRunDetail?.run.status" />
    </div>

    <div v-if="workflows.workflowRunDetail" class="workflow-run-meta">
      <div>Started: {{ formatDate(workflows.workflowRunDetail.run.started_at) }}</div>
      <div v-if="workflows.workflowRunDetail.run.finished_at">
        Finished: {{ formatDate(workflows.workflowRunDetail.run.finished_at) }}
      </div>
    </div>

    <div v-if="isTerminalRun && workflows.workflowRunDetail" class="run-summary-card">
      <div class="summary-row">
        <span class="summary-label">Final status</span>
        <StatusBadge :status="workflows.workflowRunDetail.run.status" />
      </div>
      <div v-if="runDurationText" class="summary-row">
        <span class="summary-label">Duration</span>
        <span>{{ runDurationText }}</span>
      </div>
      <div class="summary-row">
        <span class="summary-label">Nodes</span>
        <span>
          <span class="summary-chip success">{{ nodeCounts.succeeded }} ok</span>
          <span v-if="nodeCounts.failed" class="summary-chip danger"
            >{{ nodeCounts.failed }} failed</span
          >
          <span v-if="nodeCounts.canceled" class="summary-chip warning"
            >{{ nodeCounts.canceled }} canceled</span
          >
        </span>
      </div>
      <div class="summary-row">
        <button class="btn" @click="workflows.replaySelectedWorkflowRun()">
          <Icon name="replay" />
          <span>Replay in Debug</span>
        </button>
      </div>
    </div>

    <RunControlBar v-if="!isTerminalRun && workflows.workflowRunDetail" />

    <div v-if="debugState?.enabled && !isTerminalRun" class="debug-panel">
      <div class="debug-panel-header">
        <div>
          <h3>Debugger</h3>
          <span v-if="debugState.current_node_id"
            >{{ debugState.current_node_id }} · {{ debugState.current_node_kind }}</span
          >
        </div>
      </div>
      <DebugControlBar />
      <WatchExpressions />
      <details class="debug-json-group" open>
        <summary>State snapshots</summary>
        <div class="debug-grid">
          <JsonEditor :title="'Input'" :model-value="inputJsonText" readonly />
          <JsonEditor :title="'Last Output'" :model-value="lastOutputJsonText" readonly />
        </div>
      </details>
      <JsonDiff
        title="Input/output diff"
        :before="debugState.input_json ?? null"
        :after="debugState.last_output_json ?? null"
      />
      <details class="debug-json-group">
        <summary>Context JSON</summary>
        <JsonEditor
          class="debug-context-editor"
          :title="'Context'"
          :model-value="contextJsonText"
          readonly
        />
      </details>
    </div>

    <details v-if="hasNodeTiming" class="run-gantt-group" open>
      <summary>Timeline</summary>
      <RunGantt
        class="run-detail-gantt"
        :detail="workflows.workflowRunDetail"
        :selected-node-id="workflows.selectedWorkflowRunNodeId"
        @select="workflows.selectWorkflowRunNode"
      />
    </details>

    <h3 class="run-detail-section-title">Steps</h3>
    <RunTimeline
      class="run-detail-timeline"
      :detail="workflows.workflowRunDetail"
      :selected-node-id="workflows.selectedWorkflowRunNodeId"
      auto-expand-failed
      filterable
      @select="workflows.selectWorkflowRunNode"
    >
      <template #node-actions="{ node }">
        <RunNodeActions
          v-if="workflows.workflowRunDetail"
          :node="node"
          :run="workflows.workflowRunDetail.run"
          show-editor-actions
          @action="onNodeAction"
        />
      </template>
    </RunTimeline>

    <details v-if="flatSteps.length" class="flat-steps-group">
      <summary>Flat step log (debug) · {{ flatSteps.length }} step(s)</summary>
      <p class="flat-steps-hint">
        Each executed step as a flat row in creation order, with its own id and a link to the step
        that ran before it — easier to trace than the nested output tree.
      </p>
      <table class="flat-steps-table">
        <thead>
          <tr>
            <th>#</th>
            <th>Step ID</th>
            <th>Node</th>
            <th>Status</th>
            <th>Prev step</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(node, index) in flatSteps"
            :key="node.id"
            :class="{ selected: node.node_id === workflows.selectedWorkflowRunNodeId }"
            @click="workflows.selectWorkflowRunNode(node.node_id)"
          >
            <td>{{ index + 1 }}</td>
            <td class="mono" :title="node.id">{{ shortId(node.id) }}</td>
            <td>{{ node.node_id }}</td>
            <td><StatusBadge :status="node.status" /></td>
            <td class="mono">
              <button
                v-if="node.prev_node_run_id"
                type="button"
                class="prev-link"
                :title="node.prev_node_run_id"
                @click.stop="selectByRunId(node.prev_node_run_id)"
              >
                {{ shortId(node.prev_node_run_id) }}
              </button>
              <span v-else>—</span>
            </td>
          </tr>
        </tbody>
      </table>
    </details>

    <details v-if="runTransitions.length" class="transitions-group">
      <summary>Transition path · {{ runTransitions.length }} edge(s)</summary>
      <p class="transitions-hint">
        The edges this run actually walked, in order, reconstructed from each step's recorded
        origin. Untaken branches never appear here.
      </p>
      <ol class="transition-path">
        <li
          v-for="transition in runTransitions"
          :key="transition.node_run_id"
          :class="{ selected: transition.to_node === workflows.selectedWorkflowRunNodeId }"
          @click="workflows.selectWorkflowRunNode(transition.to_node)"
        >
          <span class="transition-edge">
            <span class="transition-from">{{ transition.from_node ?? "start" }}</span>
            <Icon name="chevron-right" :size="12" class="transition-arrow" />
            <span class="transition-to">{{ transition.to_node }}</span>
          </span>
          <span v-if="transition.reason" class="transition-reason">{{ transition.reason }}</span>
        </li>
      </ol>
    </details>

    <div v-if="workflows.selectedWorkflowRunNodeId" class="node-logs-section">
      <h3 class="run-detail-section-title">Result: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <div v-if="selectedNodeOutput && resultFields.length" class="result-fields">
        <div v-for="field in resultFields" :key="field.name" class="result-field-row">
          <div class="result-field-key">
            <span class="result-field-name">{{ field.label || field.name }}</span>
            <span class="result-field-type">{{ field.ty?.type ?? "any" }}</span>
          </div>
          <div
            class="result-field-value"
            :class="{ empty: selectedNodeOutput[field.name] == null }"
          >
            {{ formatResultValue(selectedNodeOutput[field.name]) }}
          </div>
        </div>
        <details v-if="hasExtraFields" class="result-extra">
          <summary>Raw JSON</summary>
          <JsonEditor
            class="workflow-detail-json"
            :model-value="selectedNodeResultText"
            readonly
            title=""
          />
        </details>
      </div>
      <JsonEditor
        v-else
        class="workflow-detail-json"
        :model-value="selectedNodeResultText"
        readonly
        title="Output JSON"
      />
      <div v-if="sortedNodeStats.length" class="node-stats">
        <h3 class="run-detail-section-title">
          Usually goes to <span class="node-stats-total">({{ nodeStatTotal }} runs)</span>
        </h3>
        <div
          v-for="stat in sortedNodeStats"
          :key="stat.to_node"
          class="node-stat-row"
          @click="workflows.selectWorkflowRunNode(stat.to_node)"
        >
          <span class="node-stat-target">{{ stat.to_node }}</span>
          <span class="node-stat-bar">
            <span class="node-stat-fill" :style="{ width: statPercent(stat.count) }" />
          </span>
          <span class="node-stat-count">{{ statPercent(stat.count) }} · {{ stat.count }}</span>
        </div>
      </div>

      <h3 class="run-detail-section-title">Logs: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <pre class="output workflow-detail-logs">{{
        workflows.workflowNodeDetailExtra || "No logs for this step"
      }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useProvidersStore } from "../../../ui/adapters/pinia/providers";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import Icon from "../shared/Icon.vue";
import StatusBadge from "../shared/StatusBadge.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import RunTimeline from "../shared/RunTimeline.vue";
import RunGantt from "../shared/RunGantt.vue";
import RunNodeActions, { type RunNodeActionType } from "../shared/RunNodeActions.vue";
import DebugControlBar from "./DebugControlBar.vue";
import RunControlBar from "./RunControlBar.vue";
import JsonDiff from "./JsonDiff.vue";
import WatchExpressions from "./WatchExpressions.vue";
import { formatDate, pretty } from "../../../core/utils/format";
import { displayValue } from "../../../core/utils/values";
import { computed, nextTick, ref, watch } from "vue";
import type {
  ActionResultMetadata,
  DebugFrame,
  NodeTransition,
  NodeTransitionStat,
  WorkflowNodeRun,
} from "../../../core/domain/models";
import {
  fetchWorkflowNodeTransitions,
  fetchWorkflowRunTransitions,
} from "../../../core/api/commandCenterApi";
import { coerceDebugFrame } from "../../../core/domain/models/workflow-state";
import {
  asArray,
  isRecord,
  workflowNodeActionConfig,
  workflowNodeResultMetadata,
} from "../../../core/workflow";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const app = useAppStore();

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
  return coerceDebugFrame(workflows.workflowRunDetail?.run.state?.debug) ?? null;
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

  if (!run?.started_at || !run.finished_at) {
    return "";
  }

  const start = Date.parse(run.started_at);
  const end = Date.parse(run.finished_at);

  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return "";
  }

  const seconds = Math.max(0, Math.round((end - start) / 1000));

  if (seconds < 60) {
    return `${String(seconds)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remSec = seconds % 60;
  return remSec === 0 ? `${String(minutes)}m` : `${String(minutes)}m ${String(remSec)}s`;
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

function formatResultValue(value: unknown): string {
  if (value === undefined || value === null) {
    return "(none)";
  }

  if (typeof value === "object") {
    return pretty(value);
  }

  return displayValue(value);
}

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

function shortId(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}

function selectByRunId(runId: string) {
  const node = workflows.workflowRunDetail?.nodes.find((item) => item.id === runId);

  if (node) {
    workflows.selectWorkflowRunNode(node.node_id);
  }
}

// audit history: the edges this run actually walked, and where the selected node usually goes.
const runTransitions = ref<NodeTransition[]>([]);
const nodeStats = ref<NodeTransitionStat[]>([]);

// reload the per-run transition path whenever a different run is opened.
watch(
  () => workflows.workflowRunDetail?.run.id ?? null,
  async (runId) => {
    runTransitions.value = [];

    if (!runId) {
      return;
    }

    try {
      runTransitions.value = await fetchWorkflowRunTransitions(runId);
    } catch {
      runTransitions.value = [];
    }
  },
  { immediate: true },
);

// reload cross-run stats for whichever node is selected.
watch(
  () => [workflows.workflowRunWorkflow?.id ?? null, workflows.selectedWorkflowRunNodeId] as const,
  async ([workflowId, nodeId]) => {
    nodeStats.value = [];

    if (!workflowId || !nodeId) {
      return;
    }

    try {
      nodeStats.value = await fetchWorkflowNodeTransitions(workflowId, nodeId);
    } catch {
      nodeStats.value = [];
    }
  },
  { immediate: true },
);

// stats sorted most-frequent-first so the common path reads at the top.
const sortedNodeStats = computed<NodeTransitionStat[]>(() =>
  [...nodeStats.value].sort((a, b) => b.count - a.count),
);

const nodeStatTotal = computed(() =>
  nodeStats.value.reduce((sum, stat) => sum + stat.count, 0),
);

function statPercent(count: number): string {
  const total = nodeStatTotal.value;

  if (total <= 0) {
    return "0%";
  }

  return `${String(Math.round((count / total) * 100))}%`;
}
</script>

