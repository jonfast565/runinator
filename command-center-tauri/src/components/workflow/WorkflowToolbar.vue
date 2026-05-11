<template>
  <div class="workflow-toolbar">
    <label>Name <input v-model="workflows.workflowDraft.name" /></label>
    <label>Version <input v-model.number="workflows.workflowDraft.version" type="number" min="1" /></label>
    <label>Concurrency <input v-model.number="workflows.workflowConcurrency" type="number" min="1" max="256" /></label>
    <button @click="workflows.saveSelectedWorkflow">Save</button>
    <button @click="workflows.addWorkflowStep">Add Step</button>
    <button :disabled="!workflows.selectedStepId" @click="workflows.removeWorkflowStep">Remove Step</button>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";

const workflows = useWorkflowsStore();
</script>
