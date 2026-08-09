<template>
  <div class="mb-2 flex flex-wrap gap-1">
    <button
      class="btn btn-sm"
      :disabled="!workflows.canPauseWorkflowRun || runControlBusy"
      title="Pause after the current node finishes"
      @click="workflows.pauseSelectedWorkflowRun"
    >
      <Icon name="pause" :size="14" />
      <span>Pause</span>
    </button>
    <button
      class="btn btn-primary btn-sm"
      :disabled="!workflows.canResumeWorkflowRun || runControlBusy"
      title="Resume a paused workflow run"
      @click="workflows.resumeSelectedWorkflowRun"
    >
      <Icon name="play" :size="14" />
      <span>Resume</span>
    </button>
    <button
      class="btn btn-danger btn-sm"
      :disabled="!workflows.canCancelWorkflowRun || runControlBusy"
      title="Cancel run immediately"
      @click="workflows.cancelSelectedWorkflowRun"
    >
      <Icon name="stop" :size="14" />
      <span>Stop</span>
    </button>
    <button
      class="btn btn-sm"
      :disabled="!workflows.canRequestRunInterrupt || runControlBusy"
      :title="
        workflows.canRequestRunInterrupt
          ? 'Ask this run to raise an interrupt on its next drive'
          : 'This workflow declares no interrupt handlers'
      "
      @click="interruptOpen = true"
    >
      <Icon name="alert" :size="14" />
      <span>Interrupt</span>
    </button>
  </div>
  <WorkflowInterruptModal v-if="interruptOpen" @close="interruptOpen = false" />
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { useOperationLoading } from "../../composables/useOperationLoading";
import Icon from "../shared/Icon.vue";
import WorkflowInterruptModal from "./WorkflowInterruptModal.vue";

const workflows = useWorkflowsStore();
const interruptOpen = ref(false);
const { isLoading: runControlBusy } = useOperationLoading(
  ["Pausing workflow run", "Resuming workflow run", "Canceling workflow run", "Requesting"],
  { prefix: true },
);
</script>
