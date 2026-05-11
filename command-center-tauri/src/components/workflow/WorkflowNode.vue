<template>
  <div class="workflow-node-content" :class="[statusClass, { 'waiting-node': isWaiting }]">
    <div class="node-label">{{ data.label }}</div>
    <div v-if="data.running" class="node-loader">
      <div class="spinner"></div>
    </div>
    
    <div v-if="isWaiting && !submitting" class="node-actions">
      <button class="node-btn approve" @click.stop="onApprove">Approve</button>
      <button class="node-btn reject" @click.stop="onReject">Reject</button>
    </div>

    <div v-if="submitting" class="node-loader">
      <div class="spinner"></div>
    </div>

    <Handle type="target" position="top" />
    <Handle type="source" position="bottom" />
  </div>
</template>

<script setup lang="ts">
import { Handle } from "@vue-flow/core";
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import { useResourcesStore } from "../../stores/resources";
import { statusClassForNode } from "../../utils/status";

const props = defineProps<{
  id: string;
  data: {
    label: string;
    running?: boolean;
    status?: string;
  };
}>();

const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const submitting = ref(false);

const statusClass = computed(() => statusClassForNode(props.data.status));

const isWaiting = computed(() => {
  const s = props.data.status;
  return s === "waiting" || s === "approval_required" || s === "approval-required" || s === "pending";
});

async function onApprove() {
  const detail = workflows.workflowRunDetail;
  if (!detail) return;
  const nodeRun = detail.nodes.find(n => n.node_id === props.id && (n.status === "waiting" || n.status === "approval_required"));
  if (nodeRun) {
    // If the node run has a task_run_id, prefer it for finding the approval record.
    // Otherwise fall back to workflow_node_run_id.
    const approval = resources.resourceRecords.find(r => 
      (nodeRun.task_run_id && r.task_run_id === nodeRun.task_run_id) || 
      (r.workflow_node_run_id === nodeRun.id)
    );
    if (approval && approval.id) {
      submitting.value = true;
      try {
        await resources.handleApprovalAction(approval.id, "approve");
        await workflows.fetchWorkflowRunDetail(detail.run.id);
      } finally {
        submitting.value = false;
      }
    }
  }
}

async function onReject() {
  const detail = workflows.workflowRunDetail;
  if (!detail) return;
  const nodeRun = detail.nodes.find(n => n.node_id === props.id && (n.status === "waiting" || n.status === "approval_required"));
  if (nodeRun) {
    // If the node run has a task_run_id, prefer it for finding the approval record.
    // Otherwise fall back to workflow_node_run_id.
    const approval = resources.resourceRecords.find(r => 
      (nodeRun.task_run_id && r.task_run_id === nodeRun.task_run_id) || 
      (r.workflow_node_run_id === nodeRun.id)
    );
    if (approval && approval.id) {
      submitting.value = true;
      try {
        await resources.handleApprovalAction(approval.id, "reject");
        await workflows.fetchWorkflowRunDetail(detail.run.id);
      } finally {
        submitting.value = false;
      }
    }
  }
}
</script>

<style scoped>
.workflow-node-content {
  padding: 10px;
  position: relative;
  min-height: 40px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  border: 1px solid transparent;
  transition: all 0.2s ease;
}

.waiting-node {
  border-width: 2px;
}

.node-label {
  text-align: center;
  font-size: 12px;
  margin-bottom: 4px;
}

.node-loader {
  position: absolute;
  top: 5px;
  right: 5px;
}

.spinner {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(0, 0, 0, 0.1);
  border-top-color: #3498db;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

.node-actions {
  display: flex;
  gap: 4px;
  margin-top: 4px;
}

.node-btn {
  font-size: 10px;
  padding: 2px 6px;
  cursor: pointer;
  border: none;
  border-radius: 3px;
  pointer-events: all;
}

.approve { background: #2ecc71; color: white; }
.reject { background: #e74c3c; color: white; }

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
