<template>
  <div class="mb-2 grid gap-1.5">
    <div class="flex flex-wrap gap-1">
      <button
        class="btn btn-primary btn-sm"
        :disabled="!workflows.canContinueWorkflowRun"
        title="Continue to the next breakpoint (F5)"
        @click="workflows.continueSelectedWorkflowRun"
      >
        <Icon name="continue" :size="14" />
        <span>Continue</span>
      </button>
      <button
        class="btn btn-sm"
        :disabled="!workflows.canStepWorkflowRun"
        title="Step one VM boundary (F10)"
        @click="workflows.stepSelectedWorkflowRun"
      >
        <Icon name="step" :size="14" />
        <span>Step</span>
      </button>
    </div>
    <div class="flex flex-wrap items-center gap-2 text-[11px] text-fg-subtle">
      <span>
        Click the red circle on any graph node to set or remove a breakpoint.
        {{ breakpointSummary }}
      </span>
      <button
        v-if="workflows.currentBreakpoints.length"
        type="button"
        class="btn btn-ghost btn-sm"
        @click="workflows.clearBreakpoints"
      >
        Clear all
      </button>
      <label
        class="inline-flex items-center gap-1.5"
        title="Pause before catch, failure, finally, or compensation routing"
      >
        <input
          type="checkbox"
          :checked="pauseOnFailure"
          @change="workflows.setPauseOnFailure(($event.target as HTMLInputElement).checked)"
        />
        Pause on failure
      </label>
    </div>
    <div v-if="selectedFailure" class="debug-failure-banner" role="status">
      <strong>{{ selectedFailure.kind ?? "failed" }}</strong>
      <span v-if="selectedFailure.nodeId">at {{ selectedFailure.nodeId }}</span>
      <span>{{ selectedFailure.message }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import Icon from "../shared/Icon.vue";

const workflows = useWorkflowsStore();
const breakpointSummary = computed(() => {
  const count = workflows.currentBreakpoints.length;
  return count === 0 ? "No breakpoints set." : `${String(count)} set.`;
});
const pauseOnFailure = computed(() => workflows.debugState?.pause_on_failure === true);
const selectedFailure = computed(() => {
  const cursorId = workflows.selectedCursorId;
  const cursor = workflows.workflowRunDetail?.vm_cursors?.find(
    (cursor) => cursor.continuation_id === cursorId,
  );
  return cursor?.pending_failure
    ? { ...cursor.pending_failure, nodeId: cursor.node_id ?? null }
    : null;
});
</script>
