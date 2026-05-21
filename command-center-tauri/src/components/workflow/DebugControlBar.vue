<template>
  <div class="debug-control-bar">
    <div class="debug-controls">
      <button
        class="debug-btn debug-btn-primary"
        :disabled="!workflows.canContinueWorkflowRun"
        title="Continue to next breakpoint (F5)"
        @click="workflows.continueSelectedWorkflowRun"
      >
        Continue
      </button>
      <button
        class="debug-btn"
        :disabled="!workflows.canStepWorkflowRun"
        title="Step over current node (F10)"
        @click="workflows.stepSelectedWorkflowRun"
      >
        Step
      </button>
      <button
        class="debug-btn"
        :disabled="!canRunToCursor"
        title="Run until selected node (Ctrl+F10)"
        @click="onRunToCursor"
      >
        To cursor
      </button>
      <button
        class="debug-btn"
        :disabled="!workflows.canStepWorkflowRun"
        title="Skip current node with synthetic output"
        @click="openSkip"
      >
        Skip…
      </button>
      <button
        class="debug-btn"
        :disabled="!workflows.canStepWorkflowRun"
        title="Re-run current node with modified input"
        @click="openRerun"
      >
        Re-run…
      </button>
      <button
        class="debug-btn debug-btn-danger"
        :disabled="!workflows.canCancelWorkflowRun"
        title="Cancel run (Shift+F5)"
        @click="workflows.cancelSelectedWorkflowRun"
      >
        Stop
      </button>
    </div>
    <div class="debug-mode-row">
      <label>
        <input
          type="radio"
          name="debug-mode"
          value="step_all"
          :checked="debugMode === 'step_all'"
          @change="onModeChange('step_all')"
        />
        Pause every node
      </label>
      <label>
        <input
          type="radio"
          name="debug-mode"
          value="breakpoints"
          :checked="debugMode === 'breakpoints'"
          @change="onModeChange('breakpoints')"
        />
        Pause at breakpoints only ({{ workflows.currentBreakpoints.length }})
      </label>
    </div>

    <DebugJsonModal
      v-if="skipOpen"
      title="Skip current node"
      hint="Provide synthetic output to record for this node. Downstream nodes will see this value as the node's output."
      editor-title="output_json"
      submit-label="Skip with this output"
      :initial-value="skipInitial"
      @close="skipOpen = false"
      @submit="onSubmitSkip"
    />
    <DebugJsonModal
      v-if="rerunOpen"
      title="Re-run current node"
      hint="Modify the parameters and re-run the current node. The prior attempt will be marked superseded."
      editor-title="parameters"
      submit-label="Re-run"
      :initial-value="rerunInitial"
      @close="rerunOpen = false"
      @submit="onSubmitRerun"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import DebugJsonModal from "./DebugJsonModal.vue";

const workflows = useWorkflowsStore();

const debugMode = computed<"step_all" | "breakpoints">(() => {
  const raw = workflows.debugState?.mode;
  return raw === "breakpoints" ? "breakpoints" : "step_all";
});

const canRunToCursor = computed(() =>
  Boolean(workflows.canContinueWorkflowRun && workflows.selectedWorkflowRunNodeId)
);

const skipOpen = ref(false);
const rerunOpen = ref(false);
const skipInitial = ref<any>({});
const rerunInitial = ref<any>({});

function onModeChange(mode: "step_all" | "breakpoints") {
  workflows.patchSelectedWorkflowRunDebug({ mode });
}

function onRunToCursor() {
  const nodeId = workflows.selectedWorkflowRunNodeId;
  if (!nodeId) return;
  workflows.runToCursor(nodeId);
}

function openSkip() {
  skipInitial.value = workflows.debugState?.last_output_json ?? {};
  skipOpen.value = true;
}

function openRerun() {
  rerunInitial.value = workflows.debugState?.input_json ?? {};
  rerunOpen.value = true;
}

function onSubmitSkip(value: any) {
  workflows.skipCurrentNode(value);
  skipOpen.value = false;
}

function onSubmitRerun(value: any) {
  workflows.rerunCurrentNode(value);
  rerunOpen.value = false;
}
</script>

<style scoped>
.debug-control-bar {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 8px;
}
.debug-controls {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.debug-btn {
  padding: 4px 9px;
  border: 1px solid #ccd4dd;
  background: #fff;
  border-radius: 4px;
  cursor: pointer;
  font-size: 11px;
  font-weight: 500;
}
.debug-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.debug-btn:hover:not(:disabled) {
  background: #f1f5f9;
}
.debug-btn-primary {
  background: #2563eb;
  border-color: #2563eb;
  color: #fff;
}
.debug-btn-primary:hover:not(:disabled) {
  background: #1d4ed8;
}
.debug-btn-danger {
  border-color: #dc2626;
  color: #dc2626;
}
.debug-btn-danger:hover:not(:disabled) {
  background: #fef2f2;
}
.debug-mode-row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 11px;
  color: #475569;
}
.debug-mode-row label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}
</style>
