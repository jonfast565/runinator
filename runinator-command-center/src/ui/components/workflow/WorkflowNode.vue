<template>
  <div
    class="workflow-node-content"
    :class="[
      statusClass,
      {
        'waiting-node': isWaitingState,
        'node-debug-active': isDebugActive,
        'node-breakpointed': data.debugBreakpoint,
        'node-skipped': data.skipped,
      },
    ]"
  >
    <span v-if="data.debugBreakpoint" class="breakpoint-dot" title="Breakpoint set" />
    <span v-if="data.locked" class="lock-dot" title="Locked node"
      ><Icon name="lock" :size="11"
    /></span>
    <span
      v-if="data.skipped"
      class="skip-dot"
      :class="{ shifted: data.locked }"
      title="Skipped node"
      ><Icon name="skip" :size="11"
    /></span>
    <div class="node-topline">
      <span class="node-kind">
        <Icon :name="kindIcon" :size="12" class="node-kind-icon" />
        <span>{{ kindLabel }}</span>
      </span>
      <span v-if="showNodeId" class="node-id" :title="`Step ID: ${id}`">{{ id }}</span>
      <span v-if="isWaitingState" class="node-waiting-icon" title="Waiting">
        <Icon name="hourglass" :size="12" />
      </span>
      <span v-if="data.statusLabel" class="node-status">{{ data.statusLabel }}</span>
      <span
        v-if="executionCount > 1"
        class="node-execution-count"
        :title="`Executed ${executionCount} times`"
        >{{ executionCount }}</span
      >
      <span
        v-if="kindDescription"
        class="node-info"
        role="note"
        :aria-label="`${kindLabel} node: ${kindDescription}`"
        @click.stop
      >
        <Icon name="info" :size="12" />
        <span class="node-info-pop" role="tooltip">
          <strong>{{ kindLabel }}</strong>
          {{ kindDescription }}
        </span>
      </span>
      <span
        v-if="data.validationCount"
        class="node-validation-badge"
        :class="data.validationSeverity"
        :title="validationTitle"
        >!</span
      >
    </div>
    <form
      v-if="isInlineEditing && !data.readOnly"
      class="node-inline-editor"
      @submit.prevent="applyInlineEdit"
      @keydown.esc.prevent="cancelInlineEdit"
      @click.stop
    >
      <input v-model="inlineId" aria-label="Node ID" placeholder="Step ID" />
      <input v-model="inlineValue" type="text" aria-label="Node name" placeholder="Name" />
      <div class="node-inline-actions">
        <button type="submit" class="node-icon-btn">Apply</button>
        <button type="button" class="node-icon-btn" @click="workflows.openStepEditor(id)">
          Edit
        </button>
        <button type="button" class="node-icon-btn" @click="cancelInlineEdit">Cancel</button>
      </div>
    </form>
    <template v-else>
      <div class="node-title">{{ data.title }}</div>
      <div v-if="data.summary" class="node-summary">{{ data.summary }}</div>
    </template>
    <div v-if="isWaiting && (data.approvalPrompt || data.inputPrompt)" class="node-prompt">
      {{ data.approvalPrompt || data.inputPrompt }}
    </div>
    <div v-if="gateStateText" class="node-gate-state">
      <div class="node-gate-line">
        <span class="node-gate-kind">{{ gateKindLabel }}</span>
        <span class="node-gate-status">{{ gateStatusLabel }}</span>
      </div>
      <div v-if="gateReasonText" class="node-gate-reason">{{ gateReasonText }}</div>
      <div v-else-if="isConditionGate" class="node-gate-reason">
        Condition gates are reducer-controlled.
      </div>
    </div>
    <div v-if="isNodeRunning" class="node-loader">
      <div class="spinner"></div>
    </div>

    <div
      v-if="isWaiting && isApprovalPending && !data.readOnly && !submitting"
      class="node-actions"
    >
      <button class="node-btn approve" @click.stop="onApprove">Approve</button>
      <button class="node-btn reject" @click.stop="onReject">Reject</button>
    </div>

    <form
      v-if="isSignalPending && !data.readOnly && !submitting"
      class="node-input-form"
      @submit.prevent="onSendSignal"
      @click.stop
    >
      <JsonEditor
        class="node-input-json"
        :model-value="signalPayloadDraft"
        title=""
        @update:model-value="onSignalPayloadChange"
      />
      <div class="node-actions">
        <button class="node-btn approve" type="submit">Send signal</button>
      </div>
      <div v-if="signalError" class="node-input-error">{{ signalError }}</div>
    </form>

    <form
      v-else-if="isWaiting && isInputPending && !data.readOnly && !submitting"
      class="node-input-form"
      @submit.prevent="onSubmitInput"
    >
      <JsonEditor
        class="node-input-json"
        :model-value="inputDraft"
        title=""
        @update:model-value="onInputDraftChange"
      />
      <div class="node-actions">
        <button class="node-btn approve" type="submit">Submit</button>
      </div>
      <div v-if="inputError" class="node-input-error">{{ inputError }}</div>
    </form>

    <form
      v-else-if="canResolveGate && !submitting"
      class="node-gate-form"
      @submit.prevent
      @click.stop
    >
      <input
        v-model="gateReasonDraft"
        class="node-gate-input"
        type="text"
        placeholder="Gate reason (optional)"
      />
      <div class="node-actions">
        <button class="node-btn approve" type="button" @click.stop="onResolveGate('open')">
          Open gate
        </button>
        <button class="node-btn reject" type="button" @click.stop="onResolveGate('close')">
          Close gate
        </button>
      </div>
    </form>

    <div v-if="submitting" class="node-loader">
      <div class="spinner"></div>
    </div>

    <template v-for="handle in semanticTargets" :key="handle.id">
      <Handle
        :id="handle.id"
        class="workflow-handle workflow-handle-target workflow-handle-semantic"
        type="target"
        :position="Position.Left"
      />
    </template>
    <template v-for="(handle, index) in semanticSources" :key="handle.id">
      <Handle
        :id="handle.id"
        class="workflow-handle workflow-handle-source workflow-handle-semantic"
        type="source"
        :position="Position.Right"
        :style="semanticHandleStyle(index, semanticSources.length)"
      />
      <span
        class="workflow-handle-label"
        :style="semanticLabelStyle(index, semanticSources.length)"
        >{{ handle.label }}</span
      >
    </template>
    <template v-for="handle in compassHandles" :key="handle.id">
      <Handle
        :id="handle.id"
        class="workflow-handle workflow-handle-target workflow-handle-compass"
        type="target"
        :position="handle.position"
        :style="handle.style"
      />
      <Handle
        :id="handle.id"
        class="workflow-handle workflow-handle-source workflow-handle-compass"
        type="source"
        :position="handle.position"
        :style="handle.style"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { Handle, Position } from "@vue-flow/core";
import { computed, ref, watch } from "vue";
import { useWorkflowsStore } from "../../../stores/workflows";
import { useResourcesStore } from "../../../stores/resources";
import { useAppStore } from "../../../stores/app";
import { isApprovalWaitingStatus, type ApprovalAction } from "../../../utils/approvals";
import { isInputWaitingStatus } from "../../../utils/inputs";
import { statusClassForNode } from "../../../utils/status";
import {
  workflowNodeKindIcon,
  workflowNodeKindDescription,
  workflowNodeKindLabel,
} from "../../../utils/workflows";
import { displayValue } from "../../../utils/values";
import { workflowRunExtrasService } from "../../../core/services";
import type {
  GateRecord,
  WorkflowInlineEditDescriptor,
  WorkflowSemanticHandle,
  WorkflowValidationIssue,
  WorkflowValidationSeverity,
} from "../../../types/models";
import JsonEditor from "../shared/JsonEditor.vue";
import Icon from "../shared/Icon.vue";

const props = defineProps<{
  id: string;
  selected?: boolean;
  data: {
    title: string;
    nodeId?: string;
    kind: string;
    summary?: string;
    semanticHandles?: WorkflowSemanticHandle[];
    inlineEdit?: WorkflowInlineEditDescriptor | null;
    validationCount?: number;
    validationSeverity?: WorkflowValidationSeverity;
    validationIssues?: WorkflowValidationIssue[];
    statusLabel?: string;
    executionCount?: number;
    approvalPrompt?: string;
    inputPrompt?: string;
    running?: boolean;
    status?: string;
    protected?: boolean;
    locked?: boolean;
    skipped?: boolean;
    readOnly?: boolean;
    allowGateResolution?: boolean;
    gate?: GateRecord | null;
    debugBreakpoint?: boolean;
  };
}>();

const workflows = useWorkflowsStore();
const resources = useResourcesStore();
const app = useAppStore();
const submitting = ref(false);
const inlineId = ref(props.id);
const inlineValue = ref(props.data.inlineEdit?.value ?? "");
const inputDraft = ref("{}");
const inputError = ref("");
const signalPayloadDraft = ref("{}");
const signalError = ref("");
const gateReasonDraft = ref("");

const statusClass = computed(() => statusClassForNode(props.data.status));
const kindIcon = computed(() => workflowNodeKindIcon(props.data.kind));
const kindDescription = computed(() => workflowNodeKindDescription(props.data.kind));
const kindLabel = computed(() => workflowNodeKindLabel(props.data.kind));
const executionCount = computed(() =>
  Math.max(0, Math.floor(props.data.executionCount ?? 0)),
);
const isApprovalPending = computed(() => isApprovalWaitingStatus(props.data.status));
const isInputPending = computed(() => isInputWaitingStatus(props.data.status));
const isWaitingState = computed(() =>
  ["waiting", "approval_required", "approval-required", "input_required", "pending"].includes(
    props.data.status ?? "",
  ),
);
// a parked signal node shares the generic `waiting` status with wait nodes; disambiguate by kind.
const isSignalPending = computed(
  () => props.data.kind === "signal" && props.data.status === "waiting",
);
const gate = computed(() => props.data.gate ?? null);
const gateKind = computed(() => gate.value?.kind ?? "");
const gateStatus = computed(() => gate.value?.status ?? "");
const gateKindLabel = computed(() => (gateKind.value ? `${gateKind.value} gate` : "gate"));
const gateStatusLabel = computed(() => gateStatus.value || "waiting");
const gateReasonText = computed(() => (gate.value?.reason ?? "").trim());
const gateStateText = computed(() => props.data.kind === "gate" && Boolean(gate.value));
const isConditionGate = computed(() => gateKind.value === "condition");
const canResolveGate = computed(() => {
  if (props.data.kind !== "gate") {
    return false;
  }

  if (!props.data.allowGateResolution || !gate.value?.id) {
    return false;
  }

  if (!["manual", "external"].includes(gateKind.value)) {
    return false;
  }

  return ["pending", "closed"].includes(gateStatus.value);
});

const isNodeRunning = computed(() => {
  const run = workflows.workflowRunDetail?.nodes.find((n) => n.node_id === props.id);

  if (run) {
    return run.status === "running" || run.status === "queued";
  }

  return props.data.running ?? false;
});

const isWaiting = computed(() => {
  return isApprovalPending.value || isInputPending.value;
});

const isDebugActive = computed(() => {
  const debug = workflows.debugState;

  if (!debug?.paused) {
    return false;
  }

  return debug.current_node_id === props.id;
});
// the inline mini-editor opens on double-click only, tracked separately from selection.
const isInlineEditing = computed(() => workflows.inlineEditNodeId === props.id);
// surface the step id in the topline whenever a custom display name hides it from the title.
const showNodeId = computed(() => props.data.title !== props.id);
const compassHandles = computed(() => [
  { id: "top", position: Position.Top, style: { left: "50%", top: "0" } },
  { id: "right", position: Position.Right, style: { right: "0", top: "50%" } },
  { id: "bottom", position: Position.Bottom, style: { left: "50%", bottom: "0" } },
  { id: "left", position: Position.Left, style: { left: "0", top: "50%" } },
]);
const semanticSources = computed(() =>
  (props.data.semanticHandles ?? []).filter((handle) => handle.type === "source"),
);
const semanticTargets = computed(() =>
  (props.data.semanticHandles ?? []).filter((handle) => handle.type === "target"),
);
const validationTitle = computed(() =>
  (props.data.validationIssues ?? []).map((issue) => issue.message).join("\n"),
);

watch(
  () => [props.id, props.data.inlineEdit?.value],
  () => {
    inlineId.value = props.id;
    inlineValue.value = props.data.inlineEdit?.value ?? "";
  },
);

watch(
  () => gate.value?.id,
  () => {
    gateReasonDraft.value = "";
  },
);

watch(
  () => [
    props.id,
    workflows.workflowRunDetail?.nodes.filter((node) => node.node_id === props.id).at(-1)?.status,
  ],
  () => {
    const nodeRun = workflows.workflowRunDetail?.nodes
      .filter((node) => node.node_id === props.id && isInputWaitingStatus(node.status))
      .at(-1);

    if (!nodeRun) {
      return;
    }

    inputDraft.value = formatInputDraft(nodeRun.output_json ?? nodeRun.state?.input ?? {});
    inputError.value = "";
  },
  { immediate: true },
);

function semanticHandleStyle(index: number, total: number) {
  return { right: "0", top: `${String(semanticHandleTop(index, total))}%` };
}

function semanticLabelStyle(index: number, total: number) {
  return { top: `${String(semanticHandleTop(index, total))}%` };
}

function semanticHandleTop(index: number, total: number) {
  if (total <= 1) {
    return 50;
  }

  return 18 + (64 * index) / Math.max(1, total - 1);
}

function applyInlineEdit() {
  workflows.submitInlineNodeEdit(props.id, inlineId.value, inlineValue.value);
}

function cancelInlineEdit() {
  inlineId.value = props.id;
  inlineValue.value = props.data.inlineEdit?.value ?? "";
  // close the inline form but keep the node selected for the inspector.
  workflows.inlineEditNodeId = "";
}

function onInputDraftChange(value: string) {
  inputDraft.value = value;
  inputError.value = "";
}

async function onApprove() {
  await resolveApproval("approve");
}

async function onReject() {
  await resolveApproval("reject");
}

async function resolveApproval(action: ApprovalAction) {
  const detail = workflows.workflowRunDetail;

  if (!detail) {
    app.setError("No workflow run selected");
    return;
  }

  const nodeRun = detail.nodes
    .filter((node) => node.node_id === props.id && isApprovalWaitingStatus(node.status))
    .at(-1);

  if (!nodeRun) {
    app.setError(`No pending approval found for workflow node ${props.id}`);
    return;
  }

  submitting.value = true;

  try {
    await resources.resolveWorkflowApproval(detail.run.id, props.id, nodeRun, action);
    await workflows.fetchWorkflowRunDetail(detail.run.id);
  } finally {
    submitting.value = false;
  }
}

function onSignalPayloadChange(value: string) {
  signalPayloadDraft.value = value;
  signalError.value = "";
}

async function onSendSignal() {
  const detail = workflows.workflowRunDetail;

  if (!detail) {
    app.setError("No workflow run selected");
    return;
  }

  const nodeRun = detail.nodes
    .filter((node) => node.node_id === props.id && node.status === "waiting")
    .at(-1);

  if (!nodeRun) {
    app.setError(`No waiting signal found for node ${props.id}`);
    return;
  }

  const name = displayValue(nodeRun.state?.name ?? "");

  if (!name) {
    app.setError(`Signal node ${props.id} has no signal name`);
    return;
  }

  let payload: unknown;

  try {
    payload = JSON.parse(signalPayloadDraft.value || "{}");
    signalError.value = "";
  } catch (err) {
    signalError.value = String(err);
    return;
  }

  submitting.value = true;

  try {
    await workflowRunExtrasService.deliverSignal(detail.run.id, name, payload);
    await workflows.fetchWorkflowRunDetail(detail.run.id);
  } finally {
    submitting.value = false;
  }
}

async function onSubmitInput() {
  const detail = workflows.workflowRunDetail;

  if (!detail) {
    app.setError("No workflow run selected");
    return;
  }

  const nodeRun = detail.nodes
    .filter((node) => node.node_id === props.id && isInputWaitingStatus(node.status))
    .at(-1);

  if (!nodeRun) {
    app.setError(`No pending input found for workflow node ${props.id}`);
    return;
  }

  let parsed: unknown;

  try {
    parsed = JSON.parse(inputDraft.value || "null");
    inputError.value = "";
  } catch (err) {
    inputError.value = String(err);
    return;
  }

  submitting.value = true;

  try {
    await workflowRunExtrasService.resolveInput(
      nodeRun.id,
      parsed,
      undefined,
      "Input submitted",
    );
    await workflows.fetchWorkflowRunDetail(detail.run.id);
  } finally {
    submitting.value = false;
  }
}

async function onResolveGate(action: "open" | "close") {
  const gateId = gate.value?.id ?? "";

  if (!gateId) {
    app.setError(`No gate found for workflow node ${props.id}`);
    return;
  }

  submitting.value = true;

  try {
    await workflows.resolveWorkflowRunGate(gateId, action, gateReasonDraft.value);
    gateReasonDraft.value = "";
  } finally {
    submitting.value = false;
  }
}

function formatInputDraft(value: unknown): string {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch {
    return "{}";
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
  border: 2px solid var(--surface);
  background: var(--accent);
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
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
  background: var(--text-muted);
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
  background: var(--accent-soft);
  color: var(--text-subtle);
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
  background: var(--warn-solid);
  color: #ffffff;
  font-size: 10px;
  font-weight: 700;
}

.node-validation-badge.error {
  background: var(--danger-solid);
}

.node-execution-count {
  display: inline-grid;
  min-width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-strong);
  border-radius: 50%;
  background: var(--surface);
  color: var(--text-subtle);
  font-size: 10px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1;
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
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
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
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  color: var(--text-muted);
  font-size: 10px;
  text-transform: uppercase;
}

.node-kind {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.node-waiting-icon {
  display: inline-grid;
  place-items: center;
  color: var(--info-fg);
}

.node-kind-icon {
  color: var(--accent-text);
}

.node-info {
  position: relative;
  display: inline-grid;
  place-items: center;
  color: var(--text-faint);
  cursor: help;
  pointer-events: all;
}

.node-info:hover {
  color: var(--accent-text);
}

.node-info-pop {
  position: absolute;
  bottom: calc(100% + 6px);
  right: -4px;
  z-index: 20;
  width: 180px;
  padding: 7px 9px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: var(--workflow-menu-shadow);
  color: var(--text-subtle);
  font-size: 11px;
  line-height: 1.4;
  text-align: left;
  text-transform: none;
  white-space: normal;
  opacity: 0;
  visibility: hidden;
  transition: opacity 0.12s ease;
  pointer-events: none;
}

.node-info-pop strong {
  display: block;
  margin-bottom: 2px;
  color: var(--text);
  text-transform: capitalize;
}

.node-info:hover .node-info-pop,
.node-info:focus-within .node-info-pop {
  opacity: 1;
  visibility: visible;
}

.node-id {
  flex: 1;
  overflow: hidden;
  color: var(--text-faint);
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-title {
  max-width: 100%;
  overflow: hidden;
  color: var(--text);
  font-weight: 700;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-summary,
.node-prompt {
  max-width: 100%;
  overflow: hidden;
  color: var(--text-subtle);
  font-size: 11px;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-prompt {
  color: var(--warning-fg);
}

.node-gate-state {
  width: 100%;
  margin-top: 4px;
  padding: 5px 6px;
  border: 1px solid var(--info-bg);
  border-radius: 5px;
  background: var(--info-bg);
  color: var(--info-fg);
  font-size: 10px;
  text-align: left;
  box-sizing: border-box;
}

.node-gate-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  text-transform: uppercase;
}

.node-gate-kind {
  font-weight: 700;
}

.node-gate-status {
  color: var(--success-fg);
}

.node-gate-reason {
  margin-top: 3px;
  color: var(--text);
  text-transform: none;
  word-break: break-word;
}

.node-loader {
  position: absolute;
  top: 5px;
  right: 5px;
}

.spinner {
  width: 12px;
  height: 12px;
  border: 2px solid var(--border-subtle);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

.node-actions {
  display: flex;
  gap: 4px;
  margin-top: 4px;
}

.node-input-json {
  min-height: 120px;
}

.node-input-json :deep(.json-editor-container) {
  min-height: 72px;
}

.node-input-error {
  color: var(--danger-fg);
  font-size: 11px;
}

.node-gate-form {
  display: grid;
  width: 100%;
  gap: 4px;
  margin-top: 4px;
}

.node-gate-input {
  min-width: 0;
  width: 100%;
  box-sizing: border-box;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  padding: 3px 5px;
  font-size: 11px;
  background: var(--surface);
}

.node-btn {
  font-size: 10px;
  padding: 2px 6px;
  cursor: pointer;
  border: none;
  border-radius: 3px;
  pointer-events: all;
}

.approve {
  background: var(--success-fg);
  color: #ffffff;
}
.reject {
  background: var(--danger-solid);
  color: #ffffff;
}

.breakpoint-dot {
  position: absolute;
  top: 4px;
  left: 4px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--danger-solid);
  border: 1px solid var(--surface);
  box-shadow: 0 0 0 1px var(--danger-solid);
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
  border: 1px solid var(--border-strong);
  border-radius: 50%;
  background: var(--surface);
  color: var(--text-subtle);
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
  border: 1px solid var(--info-fg);
  border-radius: 50%;
  background: var(--info-bg);
  color: var(--info-fg);
  z-index: 2;
}

.skip-dot.shifted {
  right: 24px;
}

.node-breakpointed {
  border-color: var(--danger-solid) !important;
}

.node-skipped {
  border-style: dashed;
  opacity: 0.78;
}

.node-debug-active {
  border-color: var(--warn-solid) !important;
  animation: debug-pulse 1.4s ease-in-out infinite;
}

@keyframes debug-pulse {
  0%,
  100% {
    box-shadow: 0 0 0 0 rgba(245, 158, 11, 0.7);
  }
  50% {
    box-shadow: 0 0 0 8px rgba(245, 158, 11, 0);
  }
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
