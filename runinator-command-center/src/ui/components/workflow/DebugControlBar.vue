<template>
  <div class="mb-2 flex flex-wrap gap-1">
    <button
      class="btn btn-primary btn-sm"
      :disabled="!workflows.canContinueWorkflowRun"
      title="Continue VM execution (F5)"
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
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import Icon from "../shared/Icon.vue";

const workflows = useWorkflowsStore();
</script>
