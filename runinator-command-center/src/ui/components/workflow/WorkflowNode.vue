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
    <span v-if="isNodeRunning" class="node-sheen" aria-hidden="true" />
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
      <!-- a handler region is unreachable from start by design, so without this it reads on the
           canvas as an orphaned island rather than as an interrupt handler. -->
      <span
        v-if="data.interruptRegion"
        class="node-interrupt-badge"
        :title="`Interrupt handler for '${data.interruptRegion.source}'${data.interruptRegion.enabled ? '' : ' (disabled)'} (region entry ${data.interruptRegion.handler})`"
        >{{
          data.interruptEntry
            ? `⚡ ${data.interruptRegion.source}${data.interruptRegion.enabled ? "" : " · off"}`
            : "⚡"
        }}</span
      >
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
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../../ui/adapters/pinia/catalogMetadata";
import { statusClassForNode } from "../../../core/utils/status";
import {
  workflowNodeKindIcon,
  workflowNodeKindDescription,
  workflowNodeKindLabel,
} from "../../../core/workflow";
import { useWorkflowNodeRuntime } from "../../composables/useWorkflowNodeRuntime";
import JsonEditor from "../shared/JsonEditor.vue";
import Icon from "../shared/Icon.vue";
import type { WorkflowNodeData } from "./workflow-node-types";

const props = defineProps<{
  id: string;
  selected?: boolean;
  data: WorkflowNodeData;
}>();

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
const inlineId = ref(props.id);
const inlineValue = ref(props.data.inlineEdit?.value ?? "");
const {
  submitting,
  inputDraft,
  inputError,
  signalPayloadDraft,
  signalError,
  gateReasonDraft,
  isApprovalPending,
  isInputPending,
  isSignalPending,
  gateKindLabel,
  gateStatusLabel,
  gateReasonText,
  gateStateText,
  isConditionGate,
  canResolveGate,
  isWaiting,
  onInputDraftChange,
  onSignalPayloadChange,
  onApprove,
  onReject,
  onSendSignal,
  onSubmitInput,
  onResolveGate,
} = useWorkflowNodeRuntime(props);

const statusClass = computed(() => statusClassForNode(props.data.status));
// subscribe to the catalog so icon/label/description refresh once metadata loads after mount.
const kindIcon = computed(() => {
  void catalogMetadata.nodeKinds;
  return workflowNodeKindIcon(props.data.kind);
});
const kindDescription = computed(() => {
  void catalogMetadata.nodeKinds;
  return workflowNodeKindDescription(props.data.kind);
});
const kindLabel = computed(() => {
  void catalogMetadata.nodeKinds;
  return workflowNodeKindLabel(props.data.kind);
});
const executionCount = computed(() => Math.max(0, Math.floor(props.data.executionCount ?? 0)));
const isWaitingState = computed(() =>
  ["waiting", "approval_required", "approval-required", "input_required", "pending"].includes(
    props.data.status ?? "",
  ),
);
const isNodeRunning = computed(() => {
  // `data.running` is the graph projection: it incorporates both the durable node-run history
  // and the live VM cursor. Reading workflowRunDetail here again bypasses that cursor signal and
  // leaves a node looking queued until the effect row catches up.
  return props.data.running ?? false;
});

// a node is "active" when any thread of control is parked on it. keying this to the run's single
// `current_node_id` showed only the primary cursor, so a parked branch of a fan-out was invisible.
// the cursors standing here are drawn by CursorTokens as travelling tokens above the graph, not by
// the node card; the card only needs to know whether one of them is parked on it.
const nodeCursors = computed(() => props.data.cursors ?? []);
const isDebugActive = computed(() => nodeCursors.value.some((cursor) => cursor.paused));

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
</script>

<style scoped src="./workflow-node.css"></style>
