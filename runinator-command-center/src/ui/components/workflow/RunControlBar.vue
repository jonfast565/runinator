<template>
  <div class="run-control-bar">
    <button
      class="btn btn-sm"
      :disabled="!workflows.canPauseWorkflowRun"
      title="Pause after the current node finishes"
      @click="workflows.pauseSelectedWorkflowRun"
    >
      <Icon name="pause" :size="14" />
      <span>Pause</span>
    </button>
    <button
      class="btn btn-primary btn-sm"
      :disabled="!workflows.canResumeWorkflowRun"
      title="Resume a paused workflow run"
      @click="workflows.resumeSelectedWorkflowRun"
    >
      <Icon name="play" :size="14" />
      <span>Resume</span>
    </button>
    <button
      class="btn btn-danger btn-sm"
      :disabled="!workflows.canCancelWorkflowRun"
      title="Cancel run immediately"
      @click="workflows.cancelSelectedWorkflowRun"
    >
      <Icon name="stop" :size="14" />
      <span>Stop</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../../stores/workflows";
import Icon from "../shared/Icon.vue";

const workflows = useWorkflowsStore();
</script>

<style scoped>
.run-control-bar {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-bottom: 8px;
}
</style>
