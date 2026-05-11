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
    
    <div v-if="workflows.selectedStepId" class="transition-helpers">
      <h3>Quick Transitions</h3>
      <div v-for="key in ['next', 'on_success', 'on_failure', 'on_timeout']" :key="key" class="transition-field">
        <span>{{ key }}</span>
        <select :value="workflows.getTransition(key)" @change="workflows.setTransition(key, ($event.target as HTMLSelectElement).value)">
          <option value="">(none)</option>
          <option v-for="node in workflows.workflowDraft.definition.nodes" :key="node.id" :value="node.id">
            {{ node.id }}
          </option>
        </select>
      </div>
    </div>

    <button :disabled="!workflows.selectedStepId" @click="workflows.applyStepEditor">Apply Step</button>
  </div>
</template>

<script setup lang="ts">
import { useWorkflowsStore } from "../../stores/workflows";
import JsonEditor from "../shared/JsonEditor.vue";

const workflows = useWorkflowsStore();
</script>

<style scoped>
.transition-helpers {
  margin: 12px 0;
  padding: 8px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 4px;
}
.transition-field {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 4px;
  font-size: 12px;
}
.transition-field select {
  width: 120px;
}
</style>
