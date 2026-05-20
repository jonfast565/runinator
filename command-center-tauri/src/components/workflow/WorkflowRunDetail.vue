<template>
  <div class="inspector-section">
    <div class="detail-header">
      <h2>Workflow Run #{{ workflows.workflowRunDetail?.run.id }}</h2>
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
        <button class="replay-btn" @click="workflows.replaySelectedWorkflowRun()">↻ Replay in Debug</button>
      </div>
    </div>

    <div v-if="debugState?.enabled && !isTerminalRun" class="debug-panel">
      <div class="debug-panel-header">
        <div>
          <h3>Debug</h3>
          <span v-if="debugState.current_node_id">{{ debugState.current_node_id }} · {{ debugState.current_node_kind }}</span>
        </div>
      </div>
      <DebugControlBar />
      <WatchExpressions />
      <div class="debug-grid">
        <JsonEditor :title="'Input'" :model-value="inputJsonText" readonly />
        <JsonEditor :title="'Last Output'" :model-value="lastOutputJsonText" readonly />
      </div>
      <JsonDiff :before="debugState.input_json ?? null" :after="debugState.last_output_json ?? null" />
      <JsonEditor :title="'Context'" :model-value="contextJsonText" readonly />
    </div>

    <h3>Steps</h3>
    <div class="table-scroll compact-scroll">
      <table class="compact">
        <thead>
          <tr>
            <th>Node</th>
            <th>Status</th>
            <th>Try</th>
            <th>Node Run</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="node in workflows.workflowRunDetail?.nodes" 
              :key="node.id" 
              :class="{ selected: workflows.selectedWorkflowRunNodeId === node.node_id }"
              @click="workflows.selectWorkflowRunNode(node.node_id)">
            <td>{{ node.node_id }}</td>
            <td><StatusBadge :status="node.status" /></td>
            <td>{{ node.attempt }}</td>
            <td>{{ node.id }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="workflows.selectedWorkflowRunNodeId" class="node-logs-section">
      <h3>Result: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <div v-if="selectedNodeOutput && resultFields.length" class="result-fields">
        <div v-for="field in resultFields" :key="field.name" class="result-field-row">
          <div class="result-field-key">
            <span class="result-field-name">{{ field.label || field.name }}</span>
            <span class="result-field-type">{{ field.value_type }}</span>
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
      <h3>Logs: {{ workflows.selectedWorkflowRunNodeId }}</h3>
      <pre class="output workflow-detail-logs">{{ workflows.workflowNodeDetailExtra || 'No logs for this step' }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import { useProvidersStore } from "../../stores/providers";
import StatusBadge from "../shared/StatusBadge.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import DebugControlBar from "./DebugControlBar.vue";
import JsonDiff from "./JsonDiff.vue";
import WatchExpressions from "./WatchExpressions.vue";
import { formatDate, pretty } from "../../utils/format";
import { computed } from "vue";
import type { ActionResultMetadata } from "../../types/models";
import { workflowNodeResultMetadata } from "../../utils/workflows";

const workflows = useWorkflowsStore();
const providersStore = useProvidersStore();

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
.workflow-run-meta {
  font-size: 12px;
  color: #66717e;
  margin-bottom: 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.run-message {
  padding: 4px 8px;
  background: #fff8f8;
  border: 1px solid #ffebeb;
  border-radius: 4px;
  color: #c53030;
}
.node-logs-section {
  margin-top: 16px;
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}
.debug-panel {
  border: 1px solid #d8e2ec;
  border-radius: 6px;
  padding: 10px;
  margin-bottom: 14px;
  background: #fbfcfe;
}
.debug-panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
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
  margin-bottom: 8px;
}
.debug-panel h4 {
  margin: 0 0 4px;
  font-size: 11px;
  color: #66717e;
}
.debug-panel :deep(.json-editor-container) {
  height: 180px;
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
.replay-btn {
  margin-left: auto;
  padding: 4px 12px;
  background: #fef3c7;
  border: 1px solid #f59e0b;
  color: #92400e;
  border-radius: 4px;
  cursor: pointer;
  font-weight: 600;
  font-size: 12px;
}
.replay-btn:hover {
  background: #fde68a;
}
.workflow-detail-logs {
  flex: 1;
  font-size: 11px;
}
.workflow-detail-result {
  max-height: 160px;
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
</style>
