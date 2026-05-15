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

    <div v-if="debugState?.enabled" class="debug-panel">
      <div class="debug-panel-header">
        <div>
          <h3>Debug</h3>
          <span v-if="debugState.current_node_id">{{ debugState.current_node_id }} · {{ debugState.current_node_kind }}</span>
        </div>
        <button :disabled="!workflows.canStepWorkflowRun" @click="workflows.stepSelectedWorkflowRun">Step</button>
      </div>
      <div class="debug-grid">
        <section>
          <h4>Input</h4>
          <pre class="output debug-json">{{ pretty(debugState.input_json ?? {}) }}</pre>
        </section>
        <section>
          <h4>Last Output</h4>
          <pre class="output debug-json">{{ pretty(debugState.last_output_json ?? null) }}</pre>
        </section>
      </div>
      <details>
        <summary>Context</summary>
        <pre class="output debug-json context">{{ pretty(debugState.context_json ?? {}) }}</pre>
      </details>
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
import { useTasksStore } from "../../stores/tasks";
import { useProvidersStore } from "../../stores/providers";
import StatusBadge from "../shared/StatusBadge.vue";
import { formatDate, pretty } from "../../utils/format";
import { computed } from "vue";
import type { ActionResultMetadata } from "../../types/models";

const workflows = useWorkflowsStore();
const tasksStore = useTasksStore();
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

const selectedNodeResultText = computed(() => {
  const node = workflows.workflowRunDetail?.nodes.find(item => item.node_id === workflows.selectedWorkflowRunNodeId);
  return pretty(node?.output_json ?? {});
});

const resultFields = computed<ActionResultMetadata[]>(() => {
  const nodeId = workflows.selectedWorkflowRunNodeId;
  if (!nodeId) return [];
  const definition = workflows.workflowRunWorkflow?.definition ?? workflows.workflowDraft.definition;
  const defNode = (definition?.nodes ?? []).find((n: any) => n.id === nodeId);
  if (!defNode?.task_id) return [];
  const task = tasksStore.tasks.find(t => t.id === defNode.task_id);
  if (!task) return [];
  const provider = providersStore.providers.find(p => p.name === task.action_name);
  const action = provider?.actions.find(a => a.function_name === task.action_function);
  return action?.results ?? [];
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
}
.debug-panel h4 {
  margin: 0 0 4px;
  font-size: 11px;
  color: #66717e;
}
.debug-json {
  max-height: 150px;
  font-size: 11px;
}
.debug-json.context {
  max-height: 220px;
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
