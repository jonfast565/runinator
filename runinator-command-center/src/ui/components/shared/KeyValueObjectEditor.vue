<template>
  <div class="key-value-object-editor">
    <header v-if="title" class="kv-header">
      <span>{{ title }}</span>
      <button type="button" class="kv-add-btn" @click="addRow">
        <Icon name="plus" :size="13" />
        Add
      </button>
    </header>
    <div v-if="rows.length" class="kv-rows">
      <div v-for="row in rows" :key="row.key" class="kv-row">
        <label class="kv-key-field">
          <span>Key</span>
          <input
            :value="draftKey(row.key)"
            @input="setDraftKey(row.key, ($event.target as HTMLInputElement).value)"
            @blur="commitDraftKey(row.key)"
            @keydown.enter.prevent="commitDraftKey(row.key)"
          />
          <small v-if="keyErrors[row.key]" class="kv-error">{{ keyErrors[row.key] }}</small>
        </label>
        <label class="kv-value-field">
          <span>Value</span>
          <TypedValueEditor
            :model-value="row.value"
            :ty="anyType"
            :force-expression="isWorkflowExpressionValue(row.value)"
            :expression-context="expressionContext"
            @update:model-value="setValue(row.key, $event)"
          />
        </label>
        <button type="button" class="kv-icon-btn" title="Remove field" @click="removeRow(row.key)">
          <Icon name="trash" :size="14" />
        </button>
      </div>
    </div>
    <div v-else class="kv-empty">
      <span>{{ emptyLabel }}</span>
      <button type="button" @click="addRow">
        <Icon name="plus" :size="13" />
        Add field
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import type { JsonRecord, RuninatorType } from "../../../types/models";
import type { WorkflowExpressionEditorContext } from "../../../utils/workflow-expression-completion";
import { isWorkflowExpressionValue } from "../../../utils/workflow-expression-completion";
import {
  removeObjectKey,
  renameObjectKey,
  setObjectValue,
  uniqueObjectKey,
} from "../../../utils/key-value-object";
import Icon from "./Icon.vue";
import TypedValueEditor from "./TypedValueEditor.vue";

const props = withDefaults(
  defineProps<{
    modelValue: JsonRecord;
    title?: string;
    emptyLabel?: string;
    expressionContext?: WorkflowExpressionEditorContext;
  }>(),
  {
    title: "",
    emptyLabel: "No fields configured.",
    expressionContext: undefined,
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: JsonRecord];
}>();

const anyType: RuninatorType = { type: "any" };
const draftKeys = reactive<Record<string, string>>({});
const keyErrors = reactive<Record<string, string>>({});

function omitReactiveKey(target: Record<string, string>, key: string): void {
  const rest = Object.fromEntries(Object.entries(target).filter(([entryKey]) => entryKey !== key));

  for (const existingKey of Object.keys(target)) {
    if (!(existingKey in rest)) {
      Reflect.deleteProperty(target, existingKey);
    }
  }

  Object.assign(target, rest);
}

const rows = computed(() =>
  Object.entries(props.modelValue).map(([key, value]) => ({ key, value })),
);

watch(
  () => Object.keys(props.modelValue),
  (keys) => {
    for (const key of Object.keys(draftKeys)) {
      if (!keys.includes(key)) {
        omitReactiveKey(draftKeys, key);
        omitReactiveKey(keyErrors, key);
      }
    }

    for (const key of keys) {
      draftKeys[key] ??= key;
    }
  },
  { immediate: true },
);

function draftKey(key: string): string {
  return draftKeys[key] ?? key;
}

function setDraftKey(previousKey: string, nextKey: string) {
  draftKeys[previousKey] = nextKey;
  keyErrors[previousKey] = validateDraftKey(previousKey, nextKey);
}

function commitDraftKey(previousKey: string) {
  const nextKey = draftKeys[previousKey] ?? previousKey;
  const result = renameObjectKey(props.modelValue, previousKey, nextKey);
  keyErrors[previousKey] = result.error;

  if (result.error) {
    return;
  }

  omitReactiveKey(draftKeys, previousKey);
  omitReactiveKey(keyErrors, previousKey);
  draftKeys[nextKey.trim()] = nextKey.trim();
  emit("update:modelValue", result.value);
}

function validateDraftKey(previousKey: string, nextKey: string): string {
  const trimmed = nextKey.trim();

  if (!trimmed) {
    return "Key is required";
  }

  if (
    trimmed !== previousKey &&
    Object.prototype.hasOwnProperty.call(props.modelValue, trimmed)
  ) {
    return "Key already exists";
  }

  return "";
}

function setValue(key: string, value: unknown) {
  if (keyErrors[key]) {
    return;
  }

  emit("update:modelValue", setObjectValue(props.modelValue, key, value));
}

function addRow() {
  const key = uniqueObjectKey(props.modelValue);
  emit("update:modelValue", setObjectValue(props.modelValue, key, null));
}

function removeRow(key: string) {
  omitReactiveKey(draftKeys, key);
  omitReactiveKey(keyErrors, key);
  emit("update:modelValue", removeObjectKey(props.modelValue, key));
}
</script>

<style scoped>
.key-value-object-editor {
  display: grid;
  min-width: 0;
  gap: 8px;
}

.kv-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
}

.kv-add-btn,
.kv-empty button,
.kv-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
}

.kv-rows {
  display: grid;
  gap: 8px;
}

.kv-row {
  display: grid;
  grid-template-columns: minmax(120px, 0.34fr) minmax(220px, 1fr) auto;
  gap: 8px;
  align-items: start;
}

.kv-key-field,
.kv-value-field {
  display: grid;
  min-width: 0;
  gap: 4px;
}

.kv-key-field span,
.kv-value-field span {
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 650;
}

.kv-icon-btn {
  align-self: end;
  width: 32px;
  min-height: 32px;
  padding: 0;
}

.kv-empty {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius);
  color: var(--text-muted);
  font-size: 12px;
  padding: 10px;
}

.kv-error {
  color: #c53030;
  font-size: 12px;
}

@media (max-width: 760px) {
  .kv-row {
    grid-template-columns: 1fr;
  }

  .kv-icon-btn {
    justify-self: end;
  }
}
</style>
