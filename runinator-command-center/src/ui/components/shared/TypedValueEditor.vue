<template>
  <div class="typed-value-editor" @mousedown.stop @click.stop>
    <div v-if="expressionsAllowed" class="value-mode-row">
      <button
        type="button"
        :class="{ active: !showExpressionEditor }"
        @click="setExpressionMode(false)"
      >
        Value
      </button>
      <button
        type="button"
        :class="{ active: showExpressionEditor }"
        @click="setExpressionMode(true)"
      >
        Expression
      </button>
    </div>
    <ExpressionJsonEditor
      v-if="showExpressionEditor"
      :model-value="expressionTextFor(modelValue)"
      :context="expressionContext"
      title="Expression"
      @update:model-value="setExpressionJsonValue"
    />
    <div v-else-if="typeKind === 'any'" class="any-editor">
      <select
        :value="anyValueKind"
        @change="setAnyValueKind(($event.target as HTMLSelectElement).value)"
      >
        <option value="string">string</option>
        <option value="number">number</option>
        <option value="boolean">boolean</option>
        <option value="null">null</option>
        <option value="object">object</option>
        <option value="array">array</option>
      </select>
      <input
        v-if="anyValueKind === 'string'"
        type="text"
        :value="stringValue"
        :placeholder="placeholder"
        @input="emitValue(($event.target as HTMLInputElement).value)"
      />
      <input
        v-else-if="anyValueKind === 'number'"
        type="number"
        step="any"
        :value="numberValue"
        @input="setAnyNumberValue(($event.target as HTMLInputElement).value)"
      />
      <label v-else-if="anyValueKind === 'boolean'" class="inline-boolean">
        <input
          type="checkbox"
          :checked="Boolean(modelValue)"
          @change="emitValue(($event.target as HTMLInputElement).checked)"
        />
        true
      </label>
      <span v-else-if="anyValueKind === 'null'" class="null-value">null</span>
      <ExpressionJsonEditor
        v-else
        :model-value="jsonText"
        :context="expressionContext"
        title="WDL Value"
        @update:model-value="setJsonValue"
      />
    </div>
    <input
      v-else-if="typeKind === 'string'"
      type="text"
      :value="stringValue"
      :placeholder="placeholder"
      @input="emitValue(($event.target as HTMLInputElement).value)"
    />
    <input
      v-else-if="typeKind === 'integer' || typeKind === 'number'"
      type="number"
      :step="typeKind === 'integer' ? 1 : 'any'"
      :value="numberValue"
      @input="setNumberValue(($event.target as HTMLInputElement).value)"
    />
    <input
      v-else-if="typeKind === 'boolean'"
      type="checkbox"
      :checked="Boolean(modelValue)"
      @change="emitValue(($event.target as HTMLInputElement).checked)"
    />
    <textarea
      v-else-if="isStringArray"
      :value="stringArrayText"
      placeholder="one value per line"
      @input="emitValue(splitLines(($event.target as HTMLTextAreaElement).value))"
    />
    <div v-else-if="typeKind === 'array' && arrayItemType" class="array-editor">
      <div
        v-for="(_item, index) in arrayValue"
        :key="arrayKeys[index] ?? index"
        class="collection-row"
      >
        <TypedValueEditor
          class="collection-value"
          :model-value="arrayValue[index]"
          :ty="arrayItemType"
          :allow-expressions="expressionsAllowed"
          :force-expression="isWorkflowExpressionValue(arrayValue[index])"
          :expression-context="expressionContext"
          @update:model-value="setArrayItem(index, $event)"
        />
        <button type="button" @click="removeArrayItem(index)">Remove</button>
      </div>
      <button type="button" @click="addArrayItem">Add Item</button>
    </div>
    <div v-else-if="typeKind === 'struct'" class="struct-editor">
      <label v-for="[fieldName, field] in structEntries" :key="fieldName">
        <span class="parameter-label">
          {{ fieldName }}
          <strong v-if="field.required">*</strong>
          <small>{{ describeType(field.ty) }}</small>
        </span>
        <TypedValueEditor
          :model-value="recordValue[fieldName]"
          :ty="field.ty"
          :allow-expressions="expressionsAllowed"
          :force-expression="isWorkflowExpressionValue(recordValue[fieldName])"
          :expression-context="expressionContext"
          @update:model-value="setRecordField(fieldName, $event)"
        />
      </label>
      <div v-if="structAdditionalType" class="map-editor">
        <div v-for="[key, value] in additionalStructEntries" :key="key" class="collection-row">
          <input
            class="collection-key"
            :value="key"
            @input="renameRecordField(key, ($event.target as HTMLInputElement).value)"
          />
          <TypedValueEditor
            class="collection-value"
            :model-value="value"
            :ty="structAdditionalType"
            :allow-expressions="expressionsAllowed"
            :force-expression="isWorkflowExpressionValue(value)"
            :expression-context="expressionContext"
            @update:model-value="setRecordField(key, $event)"
          />
          <button type="button" @click="removeRecordField(key)">Remove</button>
        </div>
        <button type="button" @click="addRecordField">Add Field</button>
      </div>
    </div>
    <div v-else-if="typeKind === 'map' && mapValueType" class="map-editor">
      <div v-for="[key, value] in recordEntries" :key="key" class="collection-row">
        <input
          class="collection-key"
          :value="key"
          @input="renameRecordField(key, ($event.target as HTMLInputElement).value)"
        />
        <TypedValueEditor
          class="collection-value"
          :model-value="value"
          :ty="mapValueType"
          :allow-expressions="expressionsAllowed"
          :force-expression="isWorkflowExpressionValue(value)"
          :expression-context="expressionContext"
          @update:model-value="setRecordField(key, $event)"
        />
        <button type="button" @click="removeRecordField(key)">Remove</button>
      </div>
      <button type="button" @click="addRecordField">Add Entry</button>
    </div>
    <div v-else-if="typeKind === 'union' && unionVariants.length" class="union-editor">
      <select
        :value="unionVariantIndex"
        @change="selectUnionVariant(Number(($event.target as HTMLSelectElement).value))"
      >
        <option v-for="(variant, index) in unionVariants" :key="index" :value="index">
          {{ describeType(variant) }}
        </option>
      </select>
      <TypedValueEditor
        :model-value="unionValue"
        :ty="selectedUnionVariant"
        :allow-expressions="expressionsAllowed"
        :force-expression="isWorkflowExpressionValue(unionValue)"
        :expression-context="expressionContext"
        @update:model-value="emitValue($event)"
      />
    </div>
    <ExpressionJsonEditor
      v-else
      :model-value="jsonText"
      :context="expressionContext"
      title="WDL Value"
      @update:model-value="setJsonValue"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { JsonRecord, RuninatorField, RuninatorType } from "../../../core/domain/models";
import { pretty } from "../../../core/utils/format";
import type { WorkflowExpressionEditorContext } from "../../../ui/adapters/codemirror/workflow-expression-completion";
import { isWorkflowExpressionValue } from "../../../ui/adapters/codemirror/workflow-expression-completion";
import ExpressionJsonEditor from "./ExpressionJsonEditor.vue";

defineOptions({ name: "TypedValueEditor" });

const props = withDefaults(
  defineProps<{
    modelValue: unknown;
    ty: RuninatorType;
    placeholder?: string;
    allowExpressions?: boolean;
    forceExpression?: boolean;
    expressionContext?: WorkflowExpressionEditorContext;
  }>(),
  {
    allowExpressions: true,
    forceExpression: false,
    placeholder: undefined,
    expressionContext: undefined,
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: unknown];
}>();

const typeKind = computed(() => props.ty.type);
const stringValue = computed(() => (typeof props.modelValue === "string" ? props.modelValue : ""));
const numberValue = computed(() => (typeof props.modelValue === "number" ? props.modelValue : ""));
const anyValueKind = computed(() => {
  if (props.modelValue === null || props.modelValue === undefined) {
    return "null";
  }

  if (Array.isArray(props.modelValue)) {
    return "array";
  }

  if (typeof props.modelValue === "object") {
    return "object";
  }

  if (typeof props.modelValue === "number") {
    return "number";
  }

  if (typeof props.modelValue === "boolean") {
    return "boolean";
  }

  return "string";
});
const arrayValue = computed<unknown[]>(() =>
  Array.isArray(props.modelValue) ? props.modelValue : [],
);
const recordValue = computed<JsonRecord>(() =>
  isPlainRecord(props.modelValue) ? props.modelValue : {},
);
const recordEntries = computed(() => Object.entries(recordValue.value));
const arrayItemType = computed(() => (props.ty.type === "array" ? props.ty.items : null));
const mapValueType = computed(() => (props.ty.type === "map" ? props.ty.values : null));
const structEntries = computed<[string, RuninatorField][]>(() =>
  props.ty.type === "struct" ? Object.entries(props.ty.fields) : [],
);
const structFieldNames = computed(() => new Set(structEntries.value.map(([name]) => name)));
const structAdditionalType = computed(() =>
  props.ty.type === "struct" ? (props.ty.additional ?? null) : null,
);
const additionalStructEntries = computed(() =>
  recordEntries.value.filter(([key]) => !structFieldNames.value.has(key)),
);
const unionVariants = computed(() => (props.ty.type === "union" ? props.ty.variants : []));
const unionVariantIndex = computed(() =>
  selectedUnionVariantIndex(props.modelValue, unionVariants.value),
);
const selectedUnionVariant = computed(
  () => unionVariants.value[unionVariantIndex.value] ?? { type: "any" },
);
const unionValue = computed(() =>
  matchesType(props.modelValue, selectedUnionVariant.value)
    ? props.modelValue
    : defaultValueForType(selectedUnionVariant.value),
);
const isStringArray = computed(() => props.ty.type === "array" && props.ty.items.type === "string");
const stringArrayText = computed(() => arrayValue.value.join("\n"));
const jsonText = computed(() => pretty(props.modelValue ?? defaultValueForType(props.ty)));
const expressionsAllowed = computed(() => props.allowExpressions);
// the toggle is driven by an explicit local intent, seeded from the incoming
// value, so the editor never swaps modes under the user while they type.
const localExpressionMode = ref(
  props.forceExpression || isWorkflowExpressionValue(props.modelValue),
);
const showExpressionEditor = computed(() => expressionsAllowed.value && localExpressionMode.value);

// an incoming expression value (or a forced expression) latches expression mode on.
watch(
  () => props.forceExpression,
  (forced) => {
    if (forced) {
      localExpressionMode.value = true;
    }
  },
);
watch(
  () => props.modelValue,
  (value) => {
    if (isWorkflowExpressionValue(value)) {
      localExpressionMode.value = true;
    }
  },
);

// stable per-item keys so editing or removing one array entry never remounts its siblings (which
// would re-seed their expression/value mode). keys are reconciled to the array length and stay put.
const arrayKeys = ref<number[]>([]);
let nextArrayKey = 0;
watch(
  () => arrayValue.value.length,
  (length) => {
    while (arrayKeys.value.length < length) {
      arrayKeys.value.push(nextArrayKey++);
    }

    if (arrayKeys.value.length > length) {
      arrayKeys.value.length = length;
    }
  },
  { immediate: true },
);

function emitValue(value: unknown) {
  emit("update:modelValue", value);
}

// render the current value as wdl, including plain literals, so editing in
// expression mode never clobbers the value back to the default expression.
function expressionTextFor(value: unknown): string {
  return pretty(value === undefined ? defaultExpressionForType(props.ty) : value);
}

function setExpressionMode(enabled: boolean) {
  localExpressionMode.value = enabled;

  if (enabled && isEmptyValue(props.modelValue)) {
    emitValue(defaultExpressionForType(props.ty));
    return;
  }

  if (!enabled && isWorkflowExpressionValue(props.modelValue)) {
    emitValue(defaultValueForType(props.ty));
  }
}

function isEmptyValue(value: unknown): boolean {
  return value === undefined || value === null || value === "";
}

function setExpressionJsonValue(raw: string) {
  try {
    emitValue(JSON.parse(raw || "null"));
  } catch {
    // keep invalid in-progress json local to codemirror until it parses.
  }
}

function setNumberValue(raw: string) {
  if (raw.trim() === "") {
    emitValue(null);
    return;
  }

  emitValue(typeKind.value === "integer" ? Number.parseInt(raw, 10) : Number(raw));
}

function setAnyNumberValue(raw: string) {
  if (raw.trim() === "") {
    emitValue(null);
    return;
  }

  emitValue(Number(raw));
}

function setAnyValueKind(kind: string) {
  if (kind === anyValueKind.value) {
    return;
  }

  if (kind === "string") {
    emitValue("");
  } else if (kind === "number") {
    emitValue(0);
  } else if (kind === "boolean") {
    emitValue(false);
  } else if (kind === "array") {
    emitValue([]);
  } else if (kind === "object") {
    emitValue({});
  } else {
    emitValue(null);
  }
}

function setJsonValue(raw: string) {
  try {
    emitValue(JSON.parse(raw || "null"));
  } catch {
    emitValue(raw);
  }
}

function setArrayItem(index: number, value: unknown) {
  const next = [...arrayValue.value];
  next[index] = value;
  emitValue(next);
}

function addArrayItem() {
  if (!arrayItemType.value) {
    return;
  }

  emitValue([...arrayValue.value, defaultValueForType(arrayItemType.value)]);
}

function removeArrayItem(index: number) {
  // drop the removed row's key so the surviving rows keep theirs (and their editor state).
  arrayKeys.value.splice(index, 1);
  emitValue(arrayValue.value.filter((_, itemIndex) => itemIndex !== index));
}

function setRecordField(key: string, value: unknown) {
  emitValue({ ...recordValue.value, [key]: value });
}

function renameRecordField(previousKey: string, nextKey: string) {
  if (previousKey === nextKey) {
    return;
  }

  const { [previousKey]: value, ...rest } = recordValue.value;
  emitValue({ ...rest, [nextKey]: value });
}

function removeRecordField(key: string) {
  emitValue(
    Object.fromEntries(Object.entries(recordValue.value).filter(([entryKey]) => entryKey !== key)),
  );
}

function addRecordField() {
  const valueType = mapValueType.value ?? structAdditionalType.value;

  if (!valueType) {
    return;
  }

  const key = uniqueRecordKey(recordValue.value);
  setRecordField(key, defaultValueForType(valueType));
}

function selectUnionVariant(index: number) {
  const variant = unionVariants.value[index];
  emitValue(defaultValueForType(variant));
}

function splitLines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function uniqueRecordKey(record: JsonRecord): string {
  let index = 1;
  let key = "key";

  while (key in record) {
    index += 1;
    key = `key_${String(index)}`;
  }

  return key;
}

function selectedUnionVariantIndex(value: unknown, variants: RuninatorType[]): number {
  const match = variants.findIndex((variant) => matchesType(value, variant));
  return match >= 0 ? match : 0;
}

function matchesType(value: unknown, ty: RuninatorType): boolean {
  if (ty.type === "any") {
    return true;
  }

  if (ty.type === "null") {
    return value === null;
  }

  if (ty.type === "string") {
    return typeof value === "string";
  }

  if (ty.type === "boolean") {
    return typeof value === "boolean";
  }

  if (ty.type === "integer") {
    return typeof value === "number" && Number.isInteger(value);
  }

  if (ty.type === "number") {
    return typeof value === "number" && !Number.isNaN(value);
  }

  if (ty.type === "duration") {
    return typeof value === "number" && Number.isInteger(value);
  }

  if (ty.type === "enum") {
    return ty.values.some((candidate) => JSON.stringify(candidate) === JSON.stringify(value));
  }

  if (ty.type === "range") {
    return (
      matchesType(value, ty.base) &&
      (ty.min === undefined || (typeof value === "number" && value >= ty.min)) &&
      (ty.max === undefined || (typeof value === "number" && value <= ty.max))
    );
  }

  if (ty.type === "array") {
    return Array.isArray(value);
  }

  if (ty.type === "map" || ty.type === "struct") {
    return isPlainRecord(value);
  }

  return ty.variants.some((variant) => matchesType(value, variant));
}

function defaultValueForType(ty: RuninatorType): unknown {
  if (ty.type === "string") {
    return "";
  }

  if (ty.type === "boolean") {
    return false;
  }

  if (ty.type === "integer" || ty.type === "number" || ty.type === "duration") {
    return 0;
  }

  if (ty.type === "enum") {
    return ty.values[0] ?? null;
  }

  if (ty.type === "range") {
    return ty.min ?? defaultValueForType(ty.base);
  }

  if (ty.type === "array") {
    return [];
  }

  if (ty.type === "map" || ty.type === "struct") {
    return {};
  }

  if (ty.type === "union") {
    return defaultValueForType(ty.variants[0] ?? { type: "any" });
  }

  return null;
}

function defaultExpressionForType(ty: RuninatorType): JsonRecord {
  if (ty.type === "string") {
    return { $to_string: { $ref: { params: ["value"] } } };
  }

  return { $ref: { params: ["value"] } };
}

function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) {
    return "any";
  }

  if (ty.type === "array") {
    return `${describeType(ty.items, depth + 1)}[]`;
  }

  if (ty.type === "map") {
    return `map<string, ${describeType(ty.values, depth + 1)}>`;
  }

  if (ty.type === "union") {
    return ty.variants.map((variant) => describeType(variant, depth + 1)).join(" | ");
  }

  if (ty.type === "enum") {
    return `enum[${ty.values.map((value) => JSON.stringify(value)).join(", ")}]`;
  }

  if (ty.type === "range") {
    return `${describeType(ty.base, depth + 1)} range ${String(ty.min ?? "")}..${String(ty.max ?? "")}`;
  }

  if (ty.type !== "struct") {
    return ty.type;
  }

  const entries = Object.entries(ty.fields);

  if (depth > 0 || entries.length > 3) {
    return "struct";
  }

  const fields = entries
    .map(
      ([name, field]) =>
        `${name}${field.required ? "" : "?"}: ${describeType(field.ty, depth + 1)}`,
    )
    .join("; ");
  return `{ ${fields} }`;
}

function isPlainRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
</script>

<style scoped>
.struct-editor,
.map-editor,
.array-editor,
.union-editor,
.any-editor {
  display: grid;
  gap: 8px;
}

.typed-value-editor {
  min-width: 0;
}

.value-mode-row {
  display: flex;
  justify-content: flex-end;
  gap: 4px;
  margin-bottom: 4px;
}

.value-mode-row button {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-subtle);
  color: var(--text-subtle);
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
  padding: 4px 7px;
}

.value-mode-row button.active {
  border-color: var(--accent);
  background: var(--accent);
  color: #ffffff;
}

.struct-editor label {
  display: grid;
  gap: 4px;
}

.collection-row {
  display: grid;
  grid-template-columns: minmax(96px, 0.4fr) minmax(160px, 1fr) auto;
  gap: 8px;
  align-items: start;
}

.array-editor .collection-row {
  grid-template-columns: minmax(160px, 1fr) auto;
}

.collection-value {
  min-width: 0;
}

.parameter-label {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: baseline;
}

.parameter-label small {
  color: var(--text-muted);
  font-size: 12px;
}

.inline-boolean {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-subtle);
  font-size: 12px;
}

.inline-boolean input {
  width: auto;
}

.null-value {
  min-height: 32px;
  display: inline-flex;
  align-items: center;
  color: var(--text-muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
}

@media (max-width: 760px) {
  .collection-row,
  .array-editor .collection-row {
    grid-template-columns: 1fr;
  }
}
</style>
