<template>
  <div class="compensation-editor">
    <label class="checkbox">
      <input :checked="enabled" type="checkbox" @change="toggle(($event.target as HTMLInputElement).checked)" />
      Undo this step when a later step fails the run
    </label>
    <p class="hint">
      A saga rollback: once this step has succeeded, a run that later reaches <code>fail</code> calls
      the action below. Compensations run in reverse order and are best-effort — one that fails does
      not stop the unwind.
    </p>

    <template v-if="enabled">
      <div class="form-grid">
        <label>
          Provider
          <select :value="providerName" @change="setProvider(($event.target as HTMLSelectElement).value)">
            <option value="" disabled>Select provider</option>
            <option v-if="providerMissing" :value="providerName">
              {{ providerName }} (unavailable)
            </option>
            <option v-for="entry in providersStore.providers" :key="entry.name" :value="entry.name">
              {{ entry.name }}
            </option>
          </select>
        </label>
        <label>
          Action
          <select
            :value="functionName"
            :disabled="!provider"
            @change="setFunction(($event.target as HTMLSelectElement).value)"
          >
            <option value="" disabled>
              {{ provider ? "Select action function" : "Select provider first" }}
            </option>
            <option v-if="functionMissing" :value="functionName">
              {{ functionName }} (unavailable)
            </option>
            <option v-for="entry in provider?.actions ?? []" :key="entry.function_name" :value="entry.function_name">
              {{ entry.function_name }}
            </option>
          </select>
        </label>
        <label>
          Timeout (s)
          <input :value="timeoutSeconds" type="number" min="1" @input="setTimeout($event)" />
        </label>
      </div>

      <p v-if="action?.description" class="hint">{{ action.description }}</p>

      <div class="form-field">
        <span class="form-field-label">Compensation Parameters</span>
        <TypedParameterEditor
          v-if="action?.parameters?.length"
          :model-value="configuration"
          :parameters="action.parameters"
          :credential-scopes="provider?.metadata.credential_scopes ?? []"
          :expression-context="expressionContext"
          @update:model-value="setConfiguration"
        />
        <KeyValueObjectEditor
          v-else
          :model-value="configuration"
          title="Compensation Parameters"
          empty-label="No compensation parameters configured."
          :expression-context="expressionContext"
          @update:model-value="setConfiguration"
        />
      </div>

      <p v-if="!provider || !action" class="hint warn">
        Pick a registered provider action; the compiler rejects a compensation it cannot resolve.
      </p>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { JsonRecord } from "../../../core/domain/models";
import { useProvidersStore } from "../../adapters/pinia/providers";
import type { WorkflowExpressionEditorContext } from "../../adapters/codemirror/workflow-expression-completion";
import KeyValueObjectEditor from "../shared/KeyValueObjectEditor.vue";
import TypedParameterEditor from "../shared/TypedParameterEditor.vue";

// lowering writes 60 when the author declares no `.timeout()`, so a new compensation starts there
// rather than at a number the round trip would immediately change.
const DEFAULT_TIMEOUT_SECONDS = 60;

const props = defineProps<{
  modelValue: JsonRecord | null;
  expressionContext?: WorkflowExpressionEditorContext;
}>();

const emit = defineEmits<(e: "update:modelValue", value: JsonRecord | null) => void>();

const providersStore = useProvidersStore();

const enabled = computed(() => props.modelValue !== null);
const current = computed<JsonRecord>(() => props.modelValue ?? {});
const providerName = computed(() =>
  typeof current.value.provider === "string" ? current.value.provider : "",
);
const functionName = computed(() =>
  typeof current.value.function === "string" ? current.value.function : "",
);
const timeoutSeconds = computed(() =>
  typeof current.value.timeout_seconds === "number"
    ? current.value.timeout_seconds
    : DEFAULT_TIMEOUT_SECONDS,
);
const configuration = computed<JsonRecord>(() => {
  const config = current.value.configuration;
  return config && typeof config === "object" && !Array.isArray(config)
    ? (config as JsonRecord)
    : {};
});

const provider = computed(
  () => providersStore.providers.find((entry) => entry.name === providerName.value) ?? null,
);
const action = computed(
  () =>
    provider.value?.actions.find((entry) => entry.function_name === functionName.value) ?? null,
);
const providerMissing = computed(() => Boolean(providerName.value) && !provider.value);
const functionMissing = computed(
  () => Boolean(functionName.value) && Boolean(provider.value) && !action.value,
);

function patch(changes: JsonRecord) {
  emit("update:modelValue", { ...current.value, ...changes });
}

function toggle(on: boolean) {
  if (!on) {
    // clears the key outright; a `{}` left behind would lower into a compensation with no provider,
    // which the compiler rejects on the next save.
    emit("update:modelValue", null);
    return;
  }

  emit("update:modelValue", {
    provider: "",
    function: "",
    timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
    configuration: {},
  });
}

function setProvider(name: string) {
  // the function belongs to the old provider; keeping it would name an action that does not exist.
  patch({ provider: name, function: "", configuration: {} });
}

function setFunction(name: string) {
  patch({ function: name });
}

function setTimeout(event: Event) {
  const raw = Number((event.target as HTMLInputElement).value);
  patch({ timeout_seconds: Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : DEFAULT_TIMEOUT_SECONDS });
}

function setConfiguration(value: JsonRecord) {
  patch({ configuration: value });
}
</script>

<style scoped>
.compensation-editor {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.hint.warn {
  color: var(--color-warning-fg, #b80);
}
</style>
