<template>
  <div class="modal-backdrop" @click.self="workflows.closeRunInput">
    <form class="modal run-input-modal" @submit.prevent="workflows.confirmRunInput">
      <header class="modal-header">
        <div>
          <h2>Run {{ workflows.selectedWorkflow?.name }}</h2>
          <span>{{ workflows.runInputDebug ? "Debug run — provide inputs" : "Provide inputs for this run" }}</span>
        </div>
        <div class="modal-header-actions">
          <button type="submit" class="primary">{{ workflows.runInputDebug ? "Run Debug" : "Run" }}</button>
          <button type="button" @click="workflows.closeRunInput">Close</button>
        </div>
      </header>

      <section class="form-section">
        <TypedValueEditor
          v-if="inputType"
          :ty="inputType"
          :model-value="workflows.runInputDraft"
          @update:model-value="onInputChange"
        />
      </section>

      <div class="modal-actions">
        <button type="button" @click="workflows.closeRunInput">Cancel</button>
        <button type="submit" class="primary">{{ workflows.runInputDebug ? "Run Debug" : "Run" }}</button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { JsonRecord } from "../../types/models";
import { useWorkflowsStore } from "../../stores/workflows";
import TypedValueEditor from "../shared/TypedValueEditor.vue";

const workflows = useWorkflowsStore();

const inputType = computed(() => workflows.selectedWorkflowInputType);

function onInputChange(value: unknown) {
  workflows.runInputDraft = (value && typeof value === "object" && !Array.isArray(value) ? value : {}) as JsonRecord;
}
</script>

<style scoped>
.run-input-modal {
  width: min(680px, 100%);
}

.form-section {
  display: grid;
  gap: 8px;
}

.modal-header-actions {
  display: flex;
  gap: 8px;
}

button.primary {
  background: #2563eb;
  border-color: #2563eb;
  color: #fff;
}
</style>
