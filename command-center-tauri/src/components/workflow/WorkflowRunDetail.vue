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

    <h3>Steps</h3>
    <div class="table-scroll compact-scroll">
      <table class="compact">
        <thead>
          <tr>
            <th>Node</th>
            <th>Status</th>
            <th>Try</th>
            <th>Task Run</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="node in workflows.workflowRunDetail?.nodes" 
              :key="node.id" 
              :class="{ selected: workflows.selectedStepId === node.node_id }"
              @click="workflows.populateStepEditor(node.node_id)">
            <td>{{ node.node_id }}</td>
            <td><StatusBadge :status="node.status" /></td>
            <td>{{ node.attempt }}</td>
            <td>{{ node.task_run_id ?? '-' }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="workflows.selectedStepId" class="node-logs-section">
      <h3>Result: {{ workflows.selectedStepId }}</h3>
      <div v-if="selectedNodeOutput && resultFields.length" class="result-fields">
        <div v-for="field in resultFields" :key="field.name" class="result-field-row">
          <span class="result-field-name">{{ field.label || field.name }}</span>
          <span class="result-field-type">{{ field.value_type }}</span>
          <span class="result-field-value">{{ formatResultValue(selectedNodeOutput[field.name]) }}</span>
        </div>
        <details v-if="hasExtraFields" class="result-extra">
          <summary>Raw JSON</summary>
          <pre class="output workflow-detail-result">{{ selectedNodeResultText }}</pre>
        </details>
      </div>
      <pre v-else class="output workflow-detail-result">{{ selectedNodeResultText }}</pre>
      <h3>Logs: {{ workflows.selectedStepId }}</h3>
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
  const node = workflows.workflowRunDetail?.nodes.find(item => item.node_id === workflows.selectedStepId);
  const output = node?.output_json;
  if (output && typeof output === "object" && !Array.isArray(output)) return output;
  return null;
});

const selectedNodeResultText = computed(() => {
  const node = workflows.workflowRunDetail?.nodes.find(item => item.node_id === workflows.selectedStepId);
  return pretty(node?.output_json ?? {});
});

const resultFields = computed<ActionResultMetadata[]>(() => {
  const nodeId = workflows.selectedStepId;
  if (!nodeId) return [];
  const defNode = (workflows.workflowDraft.definition?.nodes ?? []).find((n: any) => n.id === nodeId);
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
  gap: 4px;
  margin-bottom: 8px;
  font-size: 12px;
}
.result-field-row {
  display: grid;
  grid-template-columns: 1fr auto 2fr;
  gap: 8px;
  align-items: baseline;
  padding: 2px 0;
  border-bottom: 1px solid #f0f0f0;
}
.result-field-name {
  font-weight: 500;
}
.result-field-type {
  color: #66717e;
  font-size: 11px;
}
.result-field-value {
  font-family: monospace;
  word-break: break-all;
}
.result-extra summary {
  cursor: pointer;
  color: #66717e;
  font-size: 11px;
  margin-top: 4px;
}
</style>
