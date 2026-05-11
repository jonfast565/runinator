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
      <h3>Logs: {{ workflows.selectedStepId }}</h3>
      <pre class="output workflow-detail-logs">{{ workflows.workflowNodeDetailExtra || 'No logs for this step' }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import StatusBadge from "../shared/StatusBadge.vue";
import { formatDate } from "../../utils/format";

const workflows = useWorkflowsStore();
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
</style>
