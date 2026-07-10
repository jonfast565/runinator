<template>
  <div
    ref="modalRoot"
    class="modal-backdrop"
    tabindex="-1"
    @keydown.esc.stop.prevent="workflows.closeRunInput"
  >
    <form class="modal run-input-modal" @submit.prevent="onSubmit">
      <header class="modal-header">
        <div>
          <h2>Run {{ workflows.selectedWorkflow?.name }}</h2>
          <span>{{
            workflows.runInputDebug ? "Debug run — provide inputs" : "Provide inputs for this run"
          }}</span>
        </div>
        <div class="modal-header-actions">
          <button type="submit" class="primary" :disabled="startingRun">
            <LoadingSpinner v-if="startingRun" size="sm" label="Starting run" />
            {{ startingRun ? "Starting…" : workflows.runInputDebug ? "Run Debug" : "Run" }}
          </button>
          <button type="button" @click="workflows.closeRunInput">Close</button>
        </div>
      </header>

      <section class="form-section">
        <RunInputForm
          ref="runInputFormRef"
          :input-type="inputType"
          :storage-key="storageKey"
          :model-value="workflows.runInputDraft"
          @update:model-value="onInputChange"
        />
      </section>

      <div class="modal-actions">
        <button type="button" @click="workflows.closeRunInput">Cancel</button>
        <button type="submit" class="primary" :disabled="startingRun">
          <LoadingSpinner v-if="startingRun" size="sm" label="Starting run" />
          {{ startingRun ? "Starting…" : workflows.runInputDebug ? "Run Debug" : "Run" }}
        </button>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { JsonRecord, RuninatorType } from "../../../core/domain/models";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import RunInputForm from "../shared/RunInputForm.vue";
import LoadingSpinner from "../shared/LoadingSpinner.vue";
import { useOperationLoading } from "../../composables/useOperationLoading";

const workflows = useWorkflowsStore();
const { isLoading: startingRun } = useOperationLoading("Running workflow", { prefix: true });
const runInputFormRef = ref<InstanceType<typeof RunInputForm> | null>(null);
const modalRoot = ref<HTMLElement | null>(null);

onMounted(() => modalRoot.value?.focus());

const inputType = computed<RuninatorType>(
  () => workflows.selectedWorkflowInputType ?? { type: "any" },
);
const storageKey = computed(
  () => workflows.selectedWorkflow?.id ?? workflows.selectedWorkflow?.name ?? "none",
);

function onInputChange(value: unknown) {
  workflows.runInputDraft =
    value && typeof value === "object" && !Array.isArray(value) ? (value as JsonRecord) : {};
}

function onSubmit() {
  runInputFormRef.value?.persistLast();
  void workflows.confirmRunInput();
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
