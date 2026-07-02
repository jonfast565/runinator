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
import RunNodeActions, { type RunNodeActionType } from "../shared/RunNodeActions.vue";
import DebugControlBar from "./DebugControlBar.vue";
import RunControlBar from "./RunControlBar.vue";
import JsonDiff from "./JsonDiff.vue";
import WatchExpressions from "./WatchExpressions.vue";
import { formatDate, pretty } from "../../../core/utils/format";
import { displayValue } from "../../../core/utils/values";
import { computed, nextTick, ref } from "vue";
import type { ActionResultMetadata, DebugFrame, WorkflowNodeRun } from "../../../core/domain/models";
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
</script>

<style scoped>
.inspector-section {
  flex: 0 0 auto;
  overflow: visible;
}

.workflow-run-meta {
  font-size: 12px;
  color: var(--text-muted);
  display: flex;
  flex-wrap: wrap;
  gap: 6px 14px;
  margin-bottom: 10px;
}
.node-logs-section {
  margin-top: 10px;
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  min-height: 0;
}
.debug-panel {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px;
  margin-bottom: 10px;
  background: var(--surface-subtle);
}
.debug-panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 6px;
}
.debug-panel-header h3 {
  margin: 0;
}
.debug-panel-header span {
  color: var(--text-muted);
  font-size: 12px;
}
.debug-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin-top: 6px;
}
.debug-json-group {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  margin-bottom: 8px;
  overflow: hidden;
  padding: 6px 8px;
}
.debug-json-group summary {
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-subtle);
  user-select: none;
}
.debug-panel h4 {
  margin: 0 0 4px;
  font-size: 11px;
  color: var(--text-muted);
}
.debug-panel :deep(.json-editor-container) {
  height: 132px;
}
.debug-context-editor :deep(.json-editor-container) {
  height: 170px;
  margin-top: 6px;
}
.run-detail-section-title {
  margin-top: 2px;
}
.run-summary-card {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px 12px;
  margin-bottom: 14px;
  background: var(--surface-subtle);
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.summary-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--text-subtle);
}
.summary-label {
  width: 90px;
  color: var(--text-muted);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.run-detail-heading {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.rename-input {
  font-size: inherit;
  font-weight: inherit;
  padding: 4px 8px;
  min-width: 220px;
  width: auto;
}

.step-actions {
  text-align: right;
}
.summary-chip {
  display: inline-block;
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 500;
  margin-right: 6px;
}
.summary-chip.success {
  background: var(--success-bg);
  color: var(--success-fg);
}
.summary-chip.danger {
  background: var(--danger-bg);
  color: var(--danger-fg);
}
.summary-chip.warning {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
.workflow-detail-logs {
  flex: 0 0 auto;
  max-height: 220px;
  font-size: 11px;
}
.workflow-detail-json {
  flex: 0 0 auto;
  min-height: 0;
  margin-bottom: 10px;
}
.workflow-detail-json :deep(.json-editor-container) {
  max-height: 200px;
}
.result-extra .workflow-detail-json {
  margin: 0;
  border: 0;
  border-radius: 0;
}
.result-fields {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  overflow: hidden;
  margin-bottom: 10px;
  font-size: 12px;
}
.result-field-row {
  display: grid;
  grid-template-columns: 160px 1fr;
  min-height: 28px;
}
.result-field-row + .result-field-row {
  border-top: 1px solid var(--border-faint);
}
.result-field-key {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  background: var(--surface-subtle);
  border-right: 1px solid var(--border-subtle);
}
.result-field-name {
  font-weight: 500;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.result-field-type {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--text-muted);
  background: var(--surface-muted);
  border-radius: 3px;
  padding: 1px 4px;
  line-height: 1.4;
}
.result-field-value {
  display: flex;
  align-items: center;
  padding: 5px 10px;
  font-family: "SFMono-Regular", Consolas, monospace;
  font-size: 11px;
  color: var(--text);
  word-break: break-all;
  background: var(--surface);
}
.result-field-value.empty {
  color: var(--text-faint);
  font-style: italic;
  font-family: inherit;
}
.result-extra {
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
}
.result-extra summary {
  cursor: pointer;
  padding: 4px 10px;
  color: var(--text-muted);
  font-size: 11px;
  user-select: none;
}
.result-extra summary:hover {
  color: var(--text-subtle);
}

.flat-steps-group {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  margin: 6px 0 12px;
  padding: 6px 8px;
}
.flat-steps-group summary {
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-subtle);
  user-select: none;
}
.flat-steps-hint {
  margin: 6px 0;
  color: var(--text-muted);
  font-size: 11px;
}
.flat-steps-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.flat-steps-table th,
.flat-steps-table td {
  text-align: left;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-faint);
}
.flat-steps-table tbody tr {
  cursor: pointer;
}
.flat-steps-table tbody tr:hover {
  background: var(--surface-subtle);
}
.flat-steps-table tbody tr.selected {
  background: var(--accent-soft);
}
.flat-steps-table .mono {
  font-family: var(--font-mono);
}
.prev-link {
  background: none;
  border: none;
  padding: 0;
  color: var(--accent);
  cursor: pointer;
  font-family: var(--font-mono);
  text-decoration: underline;
}

@media (max-width: 920px) {
  .debug-grid {
    grid-template-columns: 1fr;
  }
}

/* stack the 160px label column and let wide artifact tables scroll on phones. */
@media (max-width: 760px) {
  .result-field-row {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
