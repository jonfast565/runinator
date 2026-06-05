<template>
  <div class="inspector-section">
    <div class="detail-header">
      <h2 class="run-detail-heading">
        <template v-if="!renaming">
          <span>{{ runHeadingLabel }}</span>
          <button v-if="workflows.workflowRunDetail" class="btn btn-icon btn-ghost btn-sm" title="Rename run" @click="startRename">
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
      <div v-if="workflows.workflowRunDetail.run.finished_at">Finished: {{ formatDate(workflows.workflowRunDetail.run.finished_at) }}</div>
      <div v-if="workflows.workflowRunDetail.run.message" class="run-message">
        {{ workflows.workflowRunDetail.run.message }}
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
          <span v-if="nodeCounts.failed" class="summary-chip danger">{{ nodeCounts.failed }} failed</span>
          <span v-if="nodeCounts.canceled" class="summary-chip warning">{{ nodeCounts.canceled }} canceled</span>
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
          <span v-if="debugState.current_node_id">{{ debugState.current_node_id }} · {{ debugState.current_node_kind }}</span>
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
      <JsonDiff title="Input/output diff" :before="debugState.input_json ?? null" :after="debugState.last_output_json ?? null" />
      <details class="debug-json-group">
        <summary>Context JSON</summary>
        <JsonEditor class="debug-context-editor" :title="'Context'" :model-value="contextJsonText" readonly />
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

    <div v-if="workflows.selectedWorkflowRunNodeId" class="node-logs-section">
      <h3 class="run-detail-section-title">Result: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <div v-if="selectedNodeOutput && resultFields.length" class="result-fields">
        <div v-for="field in resultFields" :key="field.name" class="result-field-row">
          <div class="result-field-key">
            <span class="result-field-name">{{ field.label || field.name }}</span>
            <span class="result-field-type">{{ field.ty?.type ?? "any" }}</span>
          </div>
          <div class="result-field-value" :class="{ empty: selectedNodeOutput[field.name] == null }">
            {{ formatResultValue(selectedNodeOutput[field.name]) }}
          </div>
        </div>
        <details v-if="hasExtraFields" class="result-extra">
          <summary>Raw JSON</summary>
          <pre class="output workflow-detail-result">{{ selectedNodeResultText }}</pre>
        </details>
      </div>
      <pre v-else class="output workflow-detail-result">{{ selectedNodeResultText }}</pre>
      <h3 class="run-detail-section-title">Logs: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <pre class="output workflow-detail-logs">{{ workflows.workflowNodeDetailExtra || 'No logs for this step' }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import { useProvidersStore } from "../../stores/providers";
import { useAppStore } from "../../stores/app";
import Icon from "../shared/Icon.vue";
import StatusBadge from "../shared/StatusBadge.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import RunTimeline from "../shared/RunTimeline.vue";
import RunNodeActions, { type RunNodeActionType } from "../shared/RunNodeActions.vue";
import DebugControlBar from "./DebugControlBar.vue";
import RunControlBar from "./RunControlBar.vue";
import JsonDiff from "./JsonDiff.vue";
import WatchExpressions from "./WatchExpressions.vue";
import { formatDate, pretty } from "../../utils/format";
import { computed, nextTick, ref } from "vue";
import type { ActionResultMetadata, WorkflowNodeRun } from "../../types/models";
import { workflowNodeActionConfig, workflowNodeResultMetadata } from "../../utils/workflows";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();
const app = useAppStore();

const renaming = ref(false);
const renameDraft = ref("");
const renameInput = ref<HTMLInputElement | null>(null);

const runHeadingLabel = computed(() => {
  const run = workflows.workflowRunDetail?.run;
  if (!run) return "Workflow Run";
  const trimmed = run.name?.trim();
  return trimmed ? `${trimmed} (#${run.id})` : `Workflow Run #${run.id}`;
});

async function startRename() {
  const run = workflows.workflowRunDetail?.run;
  if (!run) return;
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
  if (!renaming.value) return;
  const run = workflows.workflowRunDetail?.run;
  if (!run) {
    renaming.value = false;
    return;
  }
  const next = renameDraft.value.trim();
  const previous = run.name?.trim() ?? "";
  renaming.value = false;
  if (next === previous) return;
  await workflows.renameSelectedWorkflowRun(run.id, next.length === 0 ? null : next);
}

// quick actions emitted by RunNodeActions in the timeline (feature 7).
async function onNodeAction(payload: { type: RunNodeActionType; node: WorkflowNodeRun }) {
  const run = workflows.workflowRunDetail?.run;
  if (!run) return;
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
  const nodes = workflows.workflowRunWorkflow?.definition?.nodes;
  return Array.isArray(nodes) ? nodes.find((node: any) => node.id === nodeId) ?? null : null;
}

// open the step in the workflow editor, preferring the live workflow over the run snapshot.
async function openStepInEditor(nodeId: string) {
  const workflowId = workflows.workflowRunWorkflow?.id;
  const workflow = workflows.workflows.find((item) => item.id === workflowId) ?? workflows.workflowRunWorkflow;
  if (!workflow) return;
  await workflows.selectWorkflow(workflow);
  app.activeTab = "Workflows";
  workflows.openStepEditor(nodeId);
}

// focus this node's provider/action in the providers view.
function openProviderForNode(nodeId: string) {
  const node = definitionNode(nodeId);
  if (!node) return;
  const config = workflowNodeActionConfig(node);
  if (!config.provider) return;
  providersStore.focusProviderAction(config.provider, config.action);
  app.activeTab = "Providers";
}

const selectedNodeOutput = computed<Record<string, any> | null>(() => {
  const node = workflows.workflowRunDetail?.nodes.find(item => item.node_id === workflows.selectedWorkflowRunNodeId);
  const output = node?.output_json;
  if (output && typeof output === "object" && !Array.isArray(output)) return output;
  return null;
});

const debugState = computed<Record<string, any> | null>(() => {
  const debug = workflows.workflowRunDetail?.run.state?.debug;
  if (debug && typeof debug === "object" && !Array.isArray(debug)) return debug;
  return null;
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
    if (node.status === "succeeded") counts.succeeded += 1;
    else if (node.status === "failed" || node.status === "timed_out") counts.failed += 1;
    else if (node.status === "canceled") counts.canceled += 1;
  }
  return counts;
});

const runDurationText = computed(() => {
  const run = workflows.workflowRunDetail?.run;
  if (!run?.started_at || !run.finished_at) return "";
  const start = Date.parse(run.started_at);
  const end = Date.parse(run.finished_at);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return "";
  const seconds = Math.max(0, Math.round((end - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remSec = seconds % 60;
  return remSec === 0 ? `${minutes}m` : `${minutes}m ${remSec}s`;
});

const selectedNodeResultText = computed(() => {
  const node = workflows.workflowRunDetail?.nodes.find(item => item.node_id === workflows.selectedWorkflowRunNodeId);
  return pretty(node?.output_json ?? {});
});

const resultFields = computed<ActionResultMetadata[]>(() => {
  const nodeId = workflows.selectedWorkflowRunNodeId;
  if (!nodeId) return [];
  const definition = workflows.workflowRunWorkflow?.definition ?? workflows.workflowDraft.definition;
  const defNode = (definition?.nodes ?? []).find((n: any) => n.id === nodeId);
  if (!defNode) return [];
  return workflowNodeResultMetadata(defNode, providersStore.providers);
});

const hasExtraFields = computed(() => {
  if (!selectedNodeOutput.value) return false;
  const knownNames = new Set(resultFields.value.map(f => f.name));
  return Object.keys(selectedNodeOutput.value).some(k => !knownNames.has(k));
});

function formatResultValue(value: any): string {
  if (value === undefined || value === null) return "(none)";
  if (typeof value === "object") return pretty(value);
  return String(value);
}
</script>

<style scoped>
.inspector-section {
  flex: 0 0 auto;
  overflow: visible;
}

.workflow-run-meta {
  font-size: 12px;
  color: #66717e;
  display: flex;
  flex-wrap: wrap;
  gap: 6px 14px;
  margin-bottom: 10px;
}
.run-message {
  padding: 4px 8px;
  background: #fff8f8;
  border: 1px solid #ffebeb;
  border-radius: 4px;
  color: #c53030;
}
.node-logs-section {
  margin-top: 10px;
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  min-height: 0;
}
.debug-panel {
  border: 1px solid #d8e2ec;
  border-radius: 6px;
  padding: 8px;
  margin-bottom: 10px;
  background: #fbfcfe;
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
  color: #66717e;
  font-size: 12px;
}
.debug-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin-top: 6px;
}
.debug-json-group {
  border: 1px solid #e2e8f0;
  border-radius: 6px;
  background: #fff;
  margin-bottom: 8px;
  overflow: hidden;
  padding: 6px 8px;
}
.debug-json-group summary {
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  color: #475569;
  user-select: none;
}
.debug-panel h4 {
  margin: 0 0 4px;
  font-size: 11px;
  color: #66717e;
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
  border: 1px solid #d8e2ec;
  border-radius: 6px;
  padding: 10px 12px;
  margin-bottom: 14px;
  background: #f8fafc;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.summary-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: #475569;
}
.summary-label {
  width: 90px;
  color: #64748b;
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
  background: #dcfce7;
  color: #166534;
}
.summary-chip.danger {
  background: #fee2e2;
  color: #991b1b;
}
.summary-chip.warning {
  background: #fef3c7;
  color: #92400e;
}
.workflow-detail-logs {
  flex: 0 0 auto;
  max-height: 220px;
  font-size: 11px;
}
.workflow-detail-result {
  max-height: 140px;
  font-size: 11px;
}
.result-fields {
  display: flex;
  flex-direction: column;
  border: 1px solid #e2e8f0;
  border-radius: 6px;
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
  border-top: 1px solid #f0f4f8;
}
.result-field-key {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  background: #f8fafc;
  border-right: 1px solid #e2e8f0;
}
.result-field-name {
  font-weight: 500;
  color: #374151;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.result-field-type {
  flex-shrink: 0;
  font-size: 10px;
  color: #7b8794;
  background: #e9eef4;
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
  color: #17202a;
  word-break: break-all;
  background: #fff;
}
.result-field-value.empty {
  color: #97a1ad;
  font-style: italic;
  font-family: inherit;
}
.result-extra {
  border-top: 1px solid #e2e8f0;
  background: #f8fafc;
}
.result-extra summary {
  cursor: pointer;
  padding: 4px 10px;
  color: #7b8794;
  font-size: 11px;
  user-select: none;
}
.result-extra summary:hover {
  color: #374151;
}
.result-extra pre {
  margin: 0;
  border-top: 1px solid #e2e8f0;
  border-radius: 0;
}

@media (max-width: 920px) {
  .debug-grid {
    grid-template-columns: 1fr;
  }
}
</style>
