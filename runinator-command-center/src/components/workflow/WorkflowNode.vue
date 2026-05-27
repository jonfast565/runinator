<template>
  <div class="workflow-node-content" :class="[statusClass, { 'waiting-node': isWaiting, 'node-debug-active': isDebugActive, 'node-breakpointed': data.debugBreakpoint, 'node-skipped': data.skipped }]">
    <span v-if="data.debugBreakpoint" class="breakpoint-dot" title="Breakpoint set" />
    <span v-if="data.locked" class="lock-dot" title="Locked node"><Icon name="lock" :size="11" /></span>
    <span v-if="data.skipped" class="skip-dot" :class="{ shifted: data.locked }" title="Skipped node"><Icon name="skip" :size="11" /></span>
    <div class="node-topline">
      <span class="node-kind">{{ data.kind }}</span>
      <span v-if="data.statusLabel" class="node-status">{{ data.statusLabel }}</span>
      <span v-if="data.validationCount" class="node-validation-badge" :class="data.validationSeverity" :title="validationTitle">!</span>
    </div>
    <form v-if="isSelected && !data.readOnly" class="node-inline-editor" @submit.prevent="applyInlineEdit" @keydown.esc.prevent="cancelInlineEdit" @click.stop>
      <input v-model="inlineId" aria-label="Node ID" />
      <input
        v-if="data.inlineEdit"
        v-model="inlineValue"
        :type="data.inlineEdit.valueKind === 'number' ? 'number' : 'text'"
        :aria-label="data.inlineEdit.label"
      />
      <div class="node-inline-actions">
        <button type="submit" class="node-icon-btn">Apply</button>
        <button type="button" class="node-icon-btn" @click="workflows.openStepEditor(id)">Edit</button>
        <button type="button" class="node-icon-btn" @click="cancelInlineEdit">Cancel</button>
      </div>
    </form>
    <template v-else>
      <div class="node-title">{{ data.title }}</div>
      <div v-if="data.summary" class="node-summary">{{ data.summary }}</div>
    </template>
    <div v-if="isWaiting && data.approvalPrompt" class="node-prompt">{{ data.approvalPrompt }}</div>
    <div v-if="isNodeRunning" class="node-loader">
      <div class="spinner"></div>
    </div>

    <div v-if="isWaiting && !data.readOnly && !submitting" class="node-actions">
      <button class="node-btn approve" @click.stop="onApprove">Approve</button>
      <button class="node-btn reject" @click.stop="onReject">Reject</button>
    </div>

    <div v-if="submitting" class="node-loader">
      <div class="spinner"></div>
    </div>

    <template v-for="handle in semanticTargets" :key="handle.id">
      <Handle
        class="workflow-handle workflow-handle-target workflow-handle-semantic"
        type="target"
        :id="handle.id"
        :position="Position.Left"
      />
    </template>
    <template v-for="(handle, index) in semanticSources" :key="handle.id">
      <Handle
        class="workflow-handle workflow-handle-source workflow-handle-semantic"
        type="source"
        :id="handle.id"
        :position="Position.Right"
        :style="semanticHandleStyle(index, semanticSources.length)"
      />
      <span class="workflow-handle-label" :style="semanticLabelStyle(index, semanticSources.length)">{{ handle.label }}</span>
    </template>
    <template v-for="handle in compassHandles" :key="handle.id">
      <Handle class="workflow-handle workflow-handle-target workflow-handle-compass" type="target" :id="handle.id" :position="handle.position" :style="handle.style" />
      <Handle class="workflow-handle workflow-handle-source workflow-handle-compass" type="source" :id="handle.id" :position="handle.position" :style="handle.style" />
    </template>
  </div>
</template>

<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import { computed, ref, watch } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import { useResourcesStore } from "../../stores/resources";
import { useAppStore } from "../../stores/app";
import { isApprovalWaitingStatus, type ApprovalAction } from "../../utils/approvals";
import { statusClassForNode } from "../../utils/status";
import type { WorkflowInlineEditDescriptor, WorkflowSemanticHandle, WorkflowValidationIssue, WorkflowValidationSeverity } from "../../types/models";
import Icon from "../shared/Icon.vue";

const props = defineProps<{
  id: string;
  selected?: boolean;
  data: {
    title: string;
    kind: string;
    summary?: string;
    semanticHandles?: WorkflowSemanticHandle[];
    inlineEdit?: WorkflowInlineEditDescriptor | null;
    validationCount?: number;
    validationSeverity?: WorkflowValidationSeverity;
    validationIssues?: WorkflowValidationIssue[];
    statusLabel?: string;
    approvalPrompt?: string;
    running?: boolean;
    status?: string;
    protected?: boolean;
    locked?: boolean;
    skipped?: boolean;
    readOnly?: boolean;
    debugBreakpoint?: boolean;
  };
}>();

const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const app = useAppStore();
const submitting = ref(false);
const inlineId = ref(props.id);
const inlineValue = ref(props.data.inlineEdit?.value ?? "");

const statusClass = computed(() => statusClassForNode(props.data.status));

const isNodeRunning = computed(() => {
  const run = workflows.workflowRunDetail?.nodes.find(n => n.node_id === props.id);
  if (run) return run.status === "running" || run.status === "queued";
  return props.data.running ?? false;
});

const isWaiting = computed(() => {
  return isApprovalWaitingStatus(props.data.status);
});

const isDebugActive = computed(() => {
  const debug = workflows.debugState;
  if (!debug?.paused) return false;
  return debug.current_node_id === props.id;
});
const isSelected = computed(() => workflows.selectedStepId === props.id);
const compassHandles = computed(() => [
  { id: "top", position: Position.Top, style: { left: "50%", top: "0" } },
  { id: "right", position: Position.Right, style: { right: "0", top: "50%" } },
  { id: "bottom", position: Position.Bottom, style: { left: "50%", bottom: "0" } },
  { id: "left", position: Position.Left, style: { left: "0", top: "50%" } }
]);
const semanticSources = computed(() => (props.data.semanticHandles ?? []).filter((handle) => handle.type === "source"));
const semanticTargets = computed(() => (props.data.semanticHandles ?? []).filter((handle) => handle.type === "target"));
const validationTitle = computed(() => (props.data.validationIssues ?? []).map((issue) => issue.message).join("\n"));

watch(() => [props.id, props.data.inlineEdit?.value], () => {
  inlineId.value = props.id;
  inlineValue.value = props.data.inlineEdit?.value ?? "";
});

function semanticHandleStyle(index: number, total: number) {
  return { right: "0", top: `${semanticHandleTop(index, total)}%` };
}

function semanticLabelStyle(index: number, total: number) {
  return { top: `${semanticHandleTop(index, total)}%` };
}

function semanticHandleTop(index: number, total: number) {
  if (total <= 1) return 50;
  return 18 + (64 * index) / Math.max(1, total - 1);
}

function applyInlineEdit() {
  workflows.submitInlineNodeEdit(props.id, inlineId.value, inlineValue.value);
}

function cancelInlineEdit() {
  inlineId.value = props.id;
  inlineValue.value = props.data.inlineEdit?.value ?? "";
  workflows.clearWorkflowGraphSelection();
}

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

.workflow-handle-semantic {
  opacity: 0.8;
}

.workflow-handle-compass {
  opacity: 0;
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

.workflow-handle-label {
  position: absolute;
  right: -8px;
  max-width: 72px;
  overflow: hidden;
  padding: 1px 4px;
  border-radius: 4px;
  background: #eef4ff;
  color: #34495e;
  font-size: 9px;
  opacity: 0;
  pointer-events: none;
  text-overflow: ellipsis;
  transform: translate(100%, -50%);
  white-space: nowrap;
  z-index: 3;
}

.workflow-node-content:hover .workflow-handle-label,
.vue-flow__node.selected .workflow-handle-label {
  opacity: 1;
}

.node-validation-badge {
  display: inline-grid;
  width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: #f59e0b;
  color: #ffffff;
  font-size: 10px;
  font-weight: 700;
}

.node-validation-badge.error {
  background: #dc2626;
}

.node-inline-editor {
  display: grid;
  width: 100%;
  gap: 4px;
}

.node-inline-editor input {
  min-width: 0;
  width: 100%;
  box-sizing: border-box;
  border: 1px solid #cbd5e1;
  border-radius: 4px;
  padding: 3px 5px;
  font-size: 11px;
}

.node-inline-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 4px;
}

.node-icon-btn {
  padding: 2px 5px;
  font-size: 10px;
  pointer-events: all;
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

.breakpoint-dot {
  position: absolute;
  top: 4px;
  left: 4px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: #dc2626;
  border: 1px solid #fff;
  box-shadow: 0 0 0 1px #dc2626;
  z-index: 2;
}

.lock-dot {
  position: absolute;
  top: 4px;
  right: 4px;
  display: inline-grid;
  width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  border: 1px solid #94a3b8;
  border-radius: 50%;
  background: #ffffff;
  color: #475569;
  z-index: 2;
}

.skip-dot {
  position: absolute;
  top: 4px;
  right: 4px;
  display: inline-grid;
  width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  border: 1px solid #38bdf8;
  border-radius: 50%;
  background: #ecfeff;
  color: #0369a1;
  z-index: 2;
}

.skip-dot.shifted {
  right: 24px;
}

.node-breakpointed {
  border-color: #dc2626 !important;
}

.node-skipped {
  border-style: dashed;
  opacity: 0.78;
}

.node-debug-active {
  border-color: #f59e0b !important;
  animation: debug-pulse 1.4s ease-in-out infinite;
}

@keyframes debug-pulse {
  0%, 100% { box-shadow: 0 0 0 0 rgba(245, 158, 11, 0.7); }
  50% { box-shadow: 0 0 0 8px rgba(245, 158, 11, 0); }
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
