<template>
  <div
    ref="modalRoot"
    class="modal-backdrop"
    tabindex="-1"
    @keydown.esc.stop.prevent="emit('close')"
  >
    <div class="modal simulate-modal">
      <header class="modal-header">
        <div>
          <h2>Dry run · {{ workflow.name }}</h2>
          <span>Walks the workflow with the reducer's evaluators against live config — no actions
            are published.</span>
        </div>
        <div class="modal-header-actions">
          <button type="button" :disabled="running" @click="runSimulation">
            <LoadingSpinner v-if="running" size="sm" label="Simulating" />
            {{ running ? "Simulating…" : "Re-run" }}
          </button>
          <button type="button" @click="emit('close')">Close</button>
        </div>
      </header>

      <section class="simulate-body">
        <p v-if="running" class="simulate-hint">
          <LoadingSpinner size="sm" label="Simulating" /> Simulating…
        </p>
        <p v-else-if="requestError" class="simulate-error">{{ requestError }}</p>
        <template v-else-if="preview">
          <div class="simulate-summary">
            <span class="status-pill" :class="`tone-${preview.tone}`">{{ preview.status }}</span>
            <span class="simulate-count">{{ preview.reachedCount }} nodes reached</span>
          </div>
          <p v-if="preview.error" class="simulate-error">{{ preview.error }}</p>

          <ol class="simulate-steps">
            <li v-for="(row, index) in preview.rows" :key="`${row.nodeId}-${index}`">
              <span class="step-index">{{ index + 1 }}</span>
              <span class="step-dot" :class="`tone-${row.tone}`" aria-hidden="true"></span>
              <span class="step-node">{{ row.nodeId }}</span>
              <span class="step-kind">{{ row.kind }}</span>
              <span class="step-status" :class="`tone-${row.tone}`">{{ row.status }}</span>
              <span v-if="row.branch" class="step-branch">→ {{ row.branch }}</span>
              <span v-if="row.note" class="step-note">{{ row.note }}</span>
            </li>
          </ol>

          <details v-if="preview.outputJson" class="simulate-output">
            <summary>Final output</summary>
            <pre>{{ preview.outputJson }}</pre>
          </details>
        </template>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import type { SimulationRun, WorkflowDefinition } from "../../../core/domain/models";
import { asJsonValue } from "../../../core/domain/models";
import { buildSimPreview, type SimPreview } from "../../../core/workflow/run-simulation";
import { simulateWorkflow } from "../../../core/api/commandCenterApi";
import LoadingSpinner from "../shared/LoadingSpinner.vue";

const props = defineProps<{ workflow: WorkflowDefinition; inputs?: unknown }>();
const emit = defineEmits<{ close: [] }>();

const modalRoot = ref<HTMLElement | null>(null);
const running = ref(false);
const requestError = ref<string | null>(null);
const preview = ref<SimPreview | null>(null);

async function runSimulation() {
  running.value = true;
  requestError.value = null;

  try {
    const run: SimulationRun = await simulateWorkflow({
      workflow: props.workflow,
      inputs: asJsonValue(props.inputs ?? null),
    });
    preview.value = buildSimPreview(run);
  } catch (error) {
    requestError.value = error instanceof Error ? error.message : String(error);
    preview.value = null;
  } finally {
    running.value = false;
  }
}

onMounted(() => {
  modalRoot.value?.focus();
  void runSimulation();
});
</script>

