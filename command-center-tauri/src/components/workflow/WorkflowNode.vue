<template>
  <div class="workflow-node-content" :class="[statusClass, { 'waiting-node': isWaiting }]">
    <div class="node-topline">
      <span class="node-kind">{{ data.kind }}</span>
      <span v-if="data.statusLabel" class="node-status">{{ data.statusLabel }}</span>
    </div>
    <div class="node-title">{{ data.title }}</div>
    <div v-if="data.summary" class="node-summary">{{ data.summary }}</div>
    <div v-if="isWaiting && data.approvalPrompt" class="node-prompt">{{ data.approvalPrompt }}</div>
    <div v-if="isNodeRunning" class="node-loader">
      <div class="spinner"></div>
    </div>

    <div v-if="isWaiting && !submitting" class="node-actions">
      <button class="node-btn approve" @click.stop="onApprove">Approve</button>
      <button class="node-btn reject" @click.stop="onReject">Reject</button>
    </div>

    <div v-if="submitting" class="node-loader">
      <div class="spinner"></div>
    </div>

    <template v-for="handle in compassHandles" :key="handle.id">
      <Handle
        class="workflow-handle workflow-handle-target"
        type="target"
        :id="handle.id"
        :position="handle.position"
        :style="handle.style"
      />
      <Handle
        class="workflow-handle workflow-handle-source"
        type="source"
        :id="handle.id"
        :position="handle.position"
        :style="handle.style"
      />
    </template>
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
    title: string;
    kind: string;
    summary?: string;
    statusLabel?: string;
    approvalPrompt?: string;
    running?: boolean;
    status?: string;
    protected?: boolean;
  };
}>();

const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const app = useAppStore();
const submitting = ref(false);

const statusClass = computed(() => statusClassForNode(props.data.status));

const isNodeRunning = computed(() => {
  const run = workflows.workflowRunDetail?.nodes.find(n => n.node_id === props.id);
  if (run) return run.status === "running" || run.status === "queued";
  return props.data.running ?? false;
});

const isWaiting = computed(() => {
  return isApprovalWaitingStatus(props.data.status);
});
const compassHandles = computed(() => [
  { id: "top", position: Position.Top, style: { left: "50%", top: "0" } },
  { id: "right", position: Position.Right, style: { right: "0", top: "50%" } },
  { id: "bottom", position: Position.Bottom, style: { left: "50%", bottom: "0" } },
  { id: "left", position: Position.Left, style: { left: "0", top: "50%" } }
]);

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

.workflow-handle {
  width: 11px;
  height: 11px;
  opacity: 0;
  border: 2px solid #ffffff;
  background: #3498db;
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.workflow-node-content:hover .workflow-handle,
.vue-flow__node.selected .workflow-handle {
  opacity: 1;
}

.workflow-handle-target {
  background: #7f8c8d;
}

.workflow-handle-source {
  transform: scale(0.68);
}

.waiting-node {
  border-width: 2px;
}

.node-topline {
  display: flex;
  width: 100%;
  justify-content: space-between;
  gap: 6px;
  color: #66717e;
  font-size: 10px;
  text-transform: uppercase;
}

.node-title {
  max-width: 100%;
  overflow: hidden;
  color: #17202a;
  font-weight: 700;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-summary,
.node-prompt {
  max-width: 100%;
  overflow: hidden;
  color: #4b5663;
  font-size: 11px;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-prompt {
  color: #8a5a00;
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
