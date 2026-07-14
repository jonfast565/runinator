<template>
  <div class="catalog-field-editor">
    <span class="field-label">
      {{ field.label || field.name }}
      <strong v-if="field.required">*</strong>
    </span>

    <!-- provider select: shows all registered providers. -->
    <template v-if="widget === 'provider'">
      <select :value="stringModelValue" @change="emitString(($event.target as HTMLSelectElement).value)">
        <option value="" disabled>Select provider</option>
        <option
          v-if="providerMissing"
          :value="stringModelValue"
        >
          {{ stringModelValue }} (unavailable)
        </option>
        <option
          v-for="provider in providersStore.providers"
          :key="provider.name"
          :value="provider.name"
        >
          {{ provider.name }}
        </option>
      </select>
    </template>

    <!-- action_function select: shows actions for the currently selected provider. -->
    <template v-else-if="widget === 'action_function'">
      <select
        :value="stringModelValue"
        :disabled="!currentProvider"
        @change="emitString(($event.target as HTMLSelectElement).value)"
      >
        <option value="" disabled>
          {{ currentProvider ? "Select action function" : "Select provider first" }}
        </option>
        <option v-if="actionMissing" :value="stringModelValue">
          {{ stringModelValue }} (unavailable)
        </option>
        <option
          v-for="action in currentProvider?.actions ?? []"
          :key="action.function_name"
          :value="action.function_name"
        >
          {{ action.function_name }}
        </option>
      </select>
    </template>

    <!-- subflow select: shows available workflows, stored by id. -->
    <template v-else-if="widget === 'subflow'">
      <select :value="stringModelValue" @change="emitString(($event.target as HTMLSelectElement).value)">
        <option value="">(none)</option>
        <option v-for="wf in workflows" :key="String(wf.id)" :value="String(wf.id)">
          {{ wf.name }}
        </option>
      </select>
    </template>

    <!-- workflow_name select: shows available workflows, stored by name (chaining targets resolve by name). -->
    <template v-else-if="widget === 'workflow_name'">
      <select :value="stringModelValue" @change="emitString(($event.target as HTMLSelectElement).value)">
        <option value="">(none)</option>
        <option v-for="wf in workflows" :key="String(wf.id)" :value="wf.name">
          {{ wf.name }}
        </option>
      </select>
    </template>

    <!-- cron expression text input. -->
    <template v-else-if="widget === 'cron'">
      <input
        type="text"
        :value="stringModelValue"
        placeholder="0 * * * *"
        @input="emitString(($event.target as HTMLInputElement).value)"
      />
    </template>

    <!-- duration: integer number of seconds. -->
    <template v-else-if="widget === 'duration'">
      <input
        type="number"
        min="0"
        :value="numberModelValue"
        @input="emitNumber(($event.target as HTMLInputElement).value)"
      />
    </template>

    <!-- expression / json: use the expression-aware json editor. -->
    <template v-else-if="widget === 'expression' || widget === 'json'">
      <ExpressionJsonEditor
        :model-value="jsonModelValue"
        :context="expressionContext ?? undefined"
        :title="field.label || field.name"
        @update:model-value="emitJson"
      />
    </template>

    <!-- assertions: list of {name, condition, message}. -->
    <template v-else-if="widget === 'assertions'">
      <div
        v-for="(assertion, index) in assertionList"
        :key="index"
        class="assertion-row"
      >
        <div class="assertion-row-head">
          <label>Name <input v-model="assertion.name" @change="emitAssertions" /></label>
          <button type="button" @click="removeAssertion(index)">Remove</button>
        </div>
        <div class="form-field">
          <span class="form-field-label">Condition</span>
          <ExpressionJsonEditor
            v-model="assertion.condition_json"
            :context="expressionContext ?? undefined"
            title="Assertion condition"
            @update:model-value="emitAssertions"
          />
        </div>
        <label>Message <input v-model="assertion.message" @change="emitAssertions" /></label>
      </div>
      <button type="button" @click="addAssertion">Add Assertion</button>
    </template>

    <!-- node_ref: single node select. -->
    <template v-else-if="widget === 'node_ref'">
      <select :value="stringModelValue" @change="emitNodeRef(($event.target as HTMLSelectElement).value)">
        <option value="">(none)</option>
        <option v-for="nodeId in nodeOptions" :key="nodeId" :value="nodeId">{{ nodeId }}</option>
      </select>
    </template>

    <!-- default: typed value editor for the declared field type. -->
    <template v-else>
      <TypedValueEditor
        :model-value="modelValue"
        :ty="field.ty ?? undefined"
        :expression-context="expressionContext ?? undefined"
        @update:model-value="emit('update:modelValue', $event)"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { pretty } from "../../../core/utils/format";
import { parseRequiredJson } from "../../../core/utils/json";
import { nodeRef as makeNodeRef } from "../../../core/workflow/index";
import type { NodeFieldMetadata } from "../../../core/domain/models";
import { useProvidersStore } from "../../adapters/pinia/providers";
import type { WorkflowDefinition } from "../../../core/domain/models";
import ExpressionJsonEditor from "../shared/ExpressionJsonEditor.vue";
import TypedValueEditor from "../shared/TypedValueEditor.vue";

const props = withDefaults(defineProps<{
  field: NodeFieldMetadata;
  modelValue: unknown;
  expressionContext?: object | null;
  nodeOptions?: string[];
  workflows?: WorkflowDefinition[];
  // optional: sibling values from the same node (used to look up the active provider for action_function).
  siblingValues?: Record<string, unknown>;
}>(), {
  expressionContext: null,
  nodeOptions: () => [],
  workflows: () => [],
  siblingValues: () => ({}),
});

const emit = defineEmits<(e: "update:modelValue", value: unknown) => void>();

const providersStore = useProvidersStore();

const widget = computed(() => props.field.widget ?? "");

// resolve the current provider from sibling values (for action_function widget).
const currentProvider = computed(() => {
  const providerName = props.siblingValues.provider as string | undefined;
  if (!providerName) {return null;}
  return providersStore.providers.find((p) => p.name === providerName) ?? null;
});

const stringModelValue = computed(() =>
  typeof props.modelValue === "string" ? props.modelValue : "",
);

const numberModelValue = computed(() =>
  typeof props.modelValue === "number" ? props.modelValue : 0,
);

// derive a pretty-printed json string from the raw model value for expression editors.
const jsonModelValue = computed(() =>
  props.modelValue === undefined || props.modelValue === null
    ? "null"
    : pretty(props.modelValue),
);

const providerMissing = computed(() =>
  Boolean(
    stringModelValue.value &&
    !providersStore.providers.some((p) => p.name === stringModelValue.value),
  ),
);

const actionMissing = computed(() =>
  Boolean(
    stringModelValue.value &&
    currentProvider.value &&
    !currentProvider.value.actions.some((a) => a.function_name === stringModelValue.value),
  ),
);

// assertions list state — synced from modelValue (expected to be JsonRecord[]).
interface AssertionDraft {
  name: string;
  condition_json: string;
  message: string;
}

const assertionList = ref<AssertionDraft[]>(buildAssertionList(props.modelValue));

watch(
  () => props.modelValue,
  (next) => {
    assertionList.value = buildAssertionList(next);
  },
);

function buildAssertionList(value: unknown): AssertionDraft[] {
  if (!Array.isArray(value)) {return [];}
  return value.map((item) => {
    const rec = item && typeof item === "object" && !Array.isArray(item) ? (item as Record<string, unknown>) : {};
    return {
      name: typeof rec.name === "string" ? rec.name : "",
      condition_json: pretty(rec.condition ?? true),
      message: typeof rec.message === "string" ? rec.message : "",
    };
  });
}

function emitAssertions() {
  const serialized = assertionList.value.map((a) => {
    const condition = parseRequiredJson(a.condition_json) ?? true;
    const result: Record<string, unknown> = { condition };
    if (a.name.trim()) {result.name = a.name.trim();}
    if (a.message.trim()) {result.message = a.message.trim();}
    return result;
  });
  emit("update:modelValue", serialized);
}

function addAssertion() {
  assertionList.value.push({ name: "", condition_json: pretty(true), message: "" });
  emitAssertions();
}

function removeAssertion(index: number) {
  assertionList.value.splice(index, 1);
  emitAssertions();
}

function emitString(value: string) {
  emit("update:modelValue", value);
}

function emitNumber(raw: string) {
  const n = Number(raw);
  emit("update:modelValue", Number.isFinite(n) ? n : 0);
}

function emitJson(jsonText: string) {
  const parsed = parseRequiredJson(jsonText);
  // keep the raw string form if the json is invalid so the editor stays editable.
  emit("update:modelValue", parsed !== null || jsonText.trim() === "null" ? parsed : props.modelValue);
}

function emitNodeRef(nodeId: string) {
  // store as a node-ref object when non-empty, otherwise clear.
  emit("update:modelValue", nodeId ? makeNodeRef(nodeId) : undefined);
}
</script>

<style scoped>
.catalog-field-editor {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.field-label {
  font-size: 12px;
  color: var(--text-muted);
}

.field-label strong {
  color: var(--text-error, red);
  margin-left: 2px;
}

.assertion-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  margin-bottom: 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
}

.assertion-row-head {
  display: flex;
  gap: 8px;
  align-items: end;
  justify-content: space-between;
}

.assertion-row-head label {
  flex: 1;
}
</style>
