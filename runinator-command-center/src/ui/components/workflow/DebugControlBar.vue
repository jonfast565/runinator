<template>
  <div class="mb-2 flex flex-col gap-1.5">
    <div class="flex flex-wrap gap-1">
      <button
        class="btn btn-primary btn-sm"
        :disabled="!workflows.canContinueWorkflowRun"
        title="Continue to next breakpoint (F5)"
        @click="workflows.continueSelectedWorkflowRun"
      >
        <Icon name="continue" :size="14" />
        <span>Continue</span>
      </button>
      <button
        class="btn btn-sm"
        :disabled="!workflows.canStepWorkflowRun"
        title="Step over current node (F10)"
        @click="workflows.stepSelectedWorkflowRun"
      >
        <Icon name="step" :size="14" />
        <span>Step</span>
      </button>
      <button
        class="btn btn-sm"
        :disabled="!canRunToCursor"
        title="Run until selected node (Ctrl+F10)"
        @click="onRunToCursor"
      >
        <Icon name="cursor" :size="14" />
        <span>To cursor</span>
      </button>
      <button
        class="btn btn-sm"
        :disabled="!workflows.canStepWorkflowRun"
        title="Skip current node with synthetic output"
        @click="openSkip"
      >
        <Icon name="skip" :size="14" />
        <span>Skip</span>
      </button>
      <button
        class="btn btn-sm"
        :disabled="!workflows.canStepWorkflowRun"
        title="Re-run current node with modified input"
        @click="openRerun"
      >
        <Icon name="replay" :size="14" />
        <span>Re-run</span>
      </button>
    </div>
    <div class="flex flex-wrap gap-3 text-[11px] text-fg-subtle">
      <label class="inline-flex cursor-pointer items-center gap-1">
        <input
          type="radio"
          name="debug-mode"
          value="step_all"
          :checked="debugMode === 'step_all'"
          @change="onModeChange('step_all')"
        />
        Pause every node
      </label>
      <label class="inline-flex cursor-pointer items-center gap-1">
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
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import Icon from "../shared/Icon.vue";
import DebugJsonModal from "./DebugJsonModal.vue";

const workflows = useWorkflowsStore();

const debugMode = computed<"step_all" | "breakpoints">(() => {
  const raw = workflows.debugState?.mode;
  return raw === "breakpoints" ? "breakpoints" : "step_all";
});

const canRunToCursor = computed(() =>
  Boolean(workflows.canContinueWorkflowRun && workflows.selectedWorkflowRunNodeId),
);

const skipOpen = ref(false);
const rerunOpen = ref(false);
const skipInitial = ref<unknown>({});
const rerunInitial = ref<unknown>({});

function onModeChange(mode: "step_all" | "breakpoints") {
  void workflows.patchSelectedWorkflowRunDebug({ mode });
}

function onRunToCursor() {
  const nodeId = workflows.selectedWorkflowRunNodeId;

  if (!nodeId) {
    return;
  }

  void workflows.runToCursor(nodeId);
}

function openSkip() {
  skipInitial.value = workflows.debugState?.last_output_json ?? {};
  skipOpen.value = true;
}

function openRerun() {
  rerunInitial.value = workflows.debugState?.input_json ?? {};
  rerunOpen.value = true;
}

function onSubmitSkip(value: unknown) {
  void workflows.skipCurrentNode(value);
  skipOpen.value = false;
}

function onSubmitRerun(value: unknown) {
  void workflows.rerunCurrentNode(value);
  rerunOpen.value = false;
}
</script>
