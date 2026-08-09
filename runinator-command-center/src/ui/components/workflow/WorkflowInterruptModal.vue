<template>
  <Modal title="Request interrupt" width="min(560px, 100%)" @close="emit('close')">
    <p class="section-note">
      The web service only records the request. The reducer decides on the run's next drive whether
      it can be serviced — and a refusal is silent, so nothing here guarantees the handler runs.
    </p>

    <label
      >Source
      <select v-model="source">
        <option v-for="value in declaredSources" :key="value" :value="value">
          {{ labelFor(value) }}
        </option>
      </select>
    </label>
    <p v-if="sourceDescription" class="hint">{{ sourceDescription }}</p>

    <details class="advanced-sources">
      <summary>Other sources (advanced)</summary>
      <p class="hint">
        A source this workflow declares no handler for is recorded and then dropped. Drive-matched
        sources additionally require their condition to hold on the arriving drive.
      </p>
      <select v-model="source">
        <option v-for="option in allSources" :key="option.value" :value="option.value">
          {{ option.label }}
        </option>
      </select>
    </details>

    <label
      >Target thread
      <select v-model="cursorId">
        <option value="">(any thread)</option>
        <option v-for="marker in targetableCursors" :key="marker.id" :value="marker.id">
          {{ marker.label }} — {{ marker.nodeId }}
        </option>
      </select>
    </label>

    <div>
      <JsonEditor v-model="payloadJson" title="Payload" />
      <p class="hint">The handler region reads this as <code>interrupt.payload</code>.</p>
      <p v-if="payloadError" class="hint error">{{ payloadError }}</p>
    </div>

    <template #actions>
      <button type="button" class="btn" @click="emit('close')">Cancel</button>
      <button type="button" class="btn btn-primary" :disabled="!source || busy" @click="submit">
        Record request
      </button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import { useOperationLoading } from "../../composables/useOperationLoading";
import { parseRequiredJson } from "../../../core/utils/json";
import JsonEditor from "../shared/JsonEditor.vue";
import Modal from "../shared/Modal.vue";

const emit = defineEmits<{ close: [] }>();

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
const { isLoading: busy } = useOperationLoading("Requesting", { prefix: true });

const allSources = computed(() => catalogMetadata.enumOptions("interrupt_source"));
// what this workflow actually declares a handler for, plus `external`, which is the one an operator
// is always entitled to ask for.
const declaredSources = computed(() => workflows.requestableInterruptSources);

const source = ref(declaredSources.value.includes("external") ? "external" : (declaredSources.value.at(0) ?? ""));
const cursorId = ref("");
const payloadJson = ref("{}");
const payloadError = ref("");

const sourceDescription = computed(
  () => allSources.value.find((option) => option.value === source.value)?.description ?? "",
);

// a speculative cursor is excluded from interrupts, a handler cursor cannot itself be interrupted,
// and a suspended one is already frozen behind a handler.
const targetableCursors = computed(() =>
  workflows.cursorMarkers.filter(
    (marker) => !marker.speculative && !marker.interruptSource && !marker.suspended,
  ),
);

function labelFor(value: string): string {
  return allSources.value.find((option) => option.value === value)?.label ?? value;
}

async function submit() {
  const payload = parseRequiredJson(payloadJson.value);

  if (payload === null && payloadJson.value.trim() !== "null") {
    payloadError.value = "Payload must be valid JSON";
    return;
  }

  payloadError.value = "";
  await workflows.requestSelectedRunInterrupt(source.value, payload, cursorId.value || null);
  emit("close");
}
</script>
