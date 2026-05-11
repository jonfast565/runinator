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

    <Handle type="target" :position="Position.Top" />
    <Handle type="source" :position="Position.Bottom" />
  </div>
</template>

<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import { useResourcesStore } from "../../stores/resources";
import { useAppStore } from "../../stores/app";
import { isApprovalWaitingStatus, type ApprovalAction } from "../../utils/approvals";
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
const app = useAppStore();
const submitting = ref(false);

const statusClass = computed(() => statusClassForNode(props.data.status));

const isWaiting = computed(() => {
  return isApprovalWaitingStatus(props.data.status);
});

async function onApprove() {
  await resolveApproval("approve");
}

async function onReject() {
  await resolveApproval("reject");
}

async function resolveApproval(action: ApprovalAction) {
  const detail = workflows.workflowRunDetail;
  if (!detail) return app.setError("No workflow run selected");
  const nodeRun = detail.nodes.filter((node) => node.node_id === props.id && isApprovalWaitingStatus(node.status)).at(-1);
  if (!nodeRun) return app.setError(`No pending approval found for workflow node ${props.id}`);

  submitting.value = true;
  try {
    await resources.resolveWorkflowApproval(detail.run.id, props.id, nodeRun, action);
    await workflows.fetchWorkflowRunDetail(detail.run.id);
  } finally {
    submitting.value = false;
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
