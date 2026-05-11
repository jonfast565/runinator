<template>
  <div class="step-editor">
    <h2>{{ workflows.selectedStepId || "Step" }}</h2>
    <label>Step ID <input v-model="workflows.stepEditor.id" /></label>
    <label>Task ID <input v-model.number="workflows.stepEditor.task_id" type="number" min="1" /></label>
    <label>Needs <input :value="workflows.stepNeeds" disabled /></label>
    <label>Max Attempts <input v-model.number="workflows.stepEditor.max_attempts" type="number" min="1" /></label>
    <label>Timeout Seconds <input v-model.number="workflows.stepEditor.timeout_seconds" type="number" min="0" /></label>
    <label>Parameters JSON <JsonEditor v-model="workflows.stepEditor.parameters_json" /></label>
    <label>Transitions JSON <JsonEditor v-model="workflows.stepEditor.transitions_json" /></label>
    <button :disabled="!workflows.selectedStepId" @click="workflows.applyStepEditor">Apply Step</button>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";

const workflows = useWorkflowsStore();
</script>
