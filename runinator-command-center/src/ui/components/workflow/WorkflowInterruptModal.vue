<template>
  <Modal title="Request an interrupt" width="min(640px, 100%)" @close="emit('close')">
    <template #help>
      An interrupt pauses one workflow thread, runs a dedicated handler, then follows that handler's
      Resume decision.
    </template>

    <div class="interrupt-request-explainer">
      <Icon name="alert" :size="17" />
      <div>
        <strong>What will happen</strong>
        <span>
          At the next safe point, Runinator pauses the thread you choose and starts the selected
          handler. The request cannot run if that thread finishes before it reaches a safe point.
        </span>
      </div>
    </div>

    <fieldset class="interrupt-request-section">
      <legend>1. Choose the handler to run</legend>
      <p>
        Only enabled handlers declared by this run's saved workflow are available. This requests
        the chosen handler now; it does not wait for the event described below.
      </p>
      <div class="interrupt-request-options">
        <label
          v-for="option in declaredOptions"
          :key="option.value"
          class="interrupt-request-option"
          :class="{ selected: source === option.value }"
        >
          <input v-model="source" type="radio" name="interrupt-source" :value="option.value" />
          <span>
            <strong>{{ option.label }}</strong>
            <span>{{ option.description }}</span>
            <code>Handler: {{ handlerFor(option.value) }}</code>
          </span>
        </label>
      </div>
    </fieldset>

    <fieldset class="interrupt-request-section">
      <legend>2. Choose the thread to pause</legend>
      <p>
        Handler threads, speculative branches, and threads already paused by another interrupt are
        excluded automatically.
      </p>
      <label>
        Workflow thread
        <select v-model="continuationId" :disabled="targetableCursors.length <= 1">
          <option v-for="marker in targetableCursors" :key="marker.id" :value="marker.id">
            {{ marker.label }} — currently at {{ marker.nodeId }}
          </option>
        </select>
      </label>
      <span v-if="targetableCursors.length === 1" class="hint">
        The only eligible thread is selected automatically.
      </span>
      <p v-else-if="targetableCursors.length === 0" class="hint error" role="alert">
        This run has no eligible thread to interrupt right now.
      </p>
    </fieldset>

    <details class="interrupt-request-payload">
      <summary>3. Add handler payload <span>(optional)</span></summary>
      <p>
        Supply valid JSON only when the handler expects extra context. Handler steps read it from
        <code>interrupt.payload</code>.
      </p>
      <JsonEditor v-model="payloadJson" title="Payload JSON" />
      <p v-if="payloadError" class="hint error" role="alert">{{ payloadError }}</p>
    </details>

    <div v-if="source && selectedCursor" class="interrupt-request-summary" aria-live="polite">
      <Icon name="check" :size="15" />
      <span>
        Ready to request <strong>{{ labelFor(source) }}</strong> on
        <strong>{{ selectedCursor.label }}</strong
        >. The thread is paused only when the handler actually starts.
      </span>
    </div>

    <template #actions>
      <button type="button" class="btn" @click="emit('close')">Cancel</button>
      <button type="button" class="btn btn-primary" :disabled="!canSubmit" @click="submit">
        {{ busy ? "Requesting…" : "Request interrupt" }}
      </button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useWorkflowsStore } from "../../adapters/pinia/workflows";
import { useCatalogMetadataStore } from "../../adapters/pinia/catalogMetadata";
import { useOperationLoading } from "../../composables/useOperationLoading";
import { parseRequiredJson } from "../../../core/utils/json";
import { interruptDeclarations } from "../../../core/workflow/interrupt-regions";
import Icon from "../shared/Icon.vue";
import JsonEditor from "../shared/JsonEditor.vue";
import Modal from "../shared/Modal.vue";

const emit = defineEmits<{ close: [] }>();

const workflows = useWorkflowsStore();
const catalogMetadata = useCatalogMetadataStore();
const { isLoading: busy } = useOperationLoading("Requesting", { prefix: true });

const allSources = computed(() => catalogMetadata.enumOptions("interrupt_source"));
const declaredSources = computed(() => workflows.requestableInterruptSources);
const declaredOptions = computed(() =>
  declaredSources.value.map((value) => {
    const metadata = allSources.value.find((option) => option.value === value);
    return {
      value,
      label: metadata?.label ?? value,
      description: metadata?.description ?? "Runs this workflow's declared handler.",
    };
  }),
);

const source = ref("");
const continuationId = ref("");
const payloadJson = ref("{}");

// a speculative cursor is excluded from interrupts, a handler cursor cannot itself be interrupted,
// and a suspended one is already frozen behind a handler.
const targetableCursors = computed(() =>
  workflows.cursorMarkers.filter(
    (marker) =>
      !marker.speculative && !marker.interruptSource && !marker.suspended && !marker.terminal,
  ),
);
const selectedCursor = computed(
  () => targetableCursors.value.find((marker) => marker.id === continuationId.value) ?? null,
);
const payloadError = computed(() => {
  const text = payloadJson.value.trim();

  if (!text || text === "null") {
    return "";
  }

  return parseRequiredJson(text) === null ? "Payload must be valid JSON." : "";
});
const canSubmit = computed(() =>
  Boolean(source.value && selectedCursor.value && !payloadError.value && !busy.value),
);

watch(
  declaredSources,
  (sources) => {
    if (!sources.includes(source.value)) {
      source.value = sources.includes("external") ? "external" : (sources.at(0) ?? "");
    }
  },
  { immediate: true },
);
watch(
  targetableCursors,
  (cursors) => {
    if (!cursors.some((marker) => marker.id === continuationId.value)) {
      continuationId.value =
        cursors.find((marker) => marker.selected)?.id ?? cursors.at(0)?.id ?? "";
    }
  },
  { immediate: true },
);

function labelFor(value: string): string {
  return allSources.value.find((option) => option.value === value)?.label ?? value;
}

function handlerFor(sourceValue: string): string {
  return (
    interruptDeclarations(workflows.workflowRunWorkflow).find(
      (entry) => entry.enabled && entry.source === sourceValue,
    )?.handler ?? "declared handler"
  );
}

async function submit() {
  if (!canSubmit.value) {
    return;
  }

  const text = payloadJson.value.trim();
  const payload = !text || text === "null" ? null : parseRequiredJson(text);
  const recorded = await workflows.requestSelectedRunInterrupt(
    source.value,
    payload,
    continuationId.value,
  );

  if (recorded) {
    emit("close");
  }
}
</script>
