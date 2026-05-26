<template>
  <div class="typed-value-editor">
  <input
    v-if="typeKind === 'string'"
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
    <div v-for="(_item, index) in arrayValue" :key="index" class="collection-row">
      <TypedValueEditor
        class="collection-value"
        :model-value="arrayValue[index]"
        :ty="arrayItemType"
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
        @update:model-value="setRecordField(fieldName, $event)"
      />
    </label>
    <div v-if="structAdditionalType" class="map-editor">
      <div v-for="[key, value] in additionalStructEntries" :key="key" class="collection-row">
        <input class="collection-key" :value="key" @input="renameRecordField(key, ($event.target as HTMLInputElement).value)" />
        <TypedValueEditor
          class="collection-value"
          :model-value="value"
          :ty="structAdditionalType"
          @update:model-value="setRecordField(key, $event)"
        />
        <button type="button" @click="removeRecordField(key)">Remove</button>
      </div>
      <button type="button" @click="addRecordField">Add Field</button>
    </div>
  </div>
  <div v-else-if="typeKind === 'map' && mapValueType" class="map-editor">
    <div v-for="[key, value] in recordEntries" :key="key" class="collection-row">
      <input class="collection-key" :value="key" @input="renameRecordField(key, ($event.target as HTMLInputElement).value)" />
      <TypedValueEditor
        class="collection-value"
        :model-value="value"
        :ty="mapValueType"
        @update:model-value="setRecordField(key, $event)"
      />
      <button type="button" @click="removeRecordField(key)">Remove</button>
    </div>
    <button type="button" @click="addRecordField">Add Entry</button>
  </div>
  <div v-else-if="typeKind === 'union' && unionVariants.length" class="union-editor">
    <select :value="unionVariantIndex" @change="selectUnionVariant(Number(($event.target as HTMLSelectElement).value))">
      <option v-for="(variant, index) in unionVariants" :key="index" :value="index">
        {{ describeType(variant) }}
      </option>
    </select>
    <TypedValueEditor
      :model-value="unionValue"
      :ty="selectedUnionVariant"
      @update:model-value="emitValue($event)"
    />
  </div>
  <JsonEditor
    v-else
    :model-value="jsonText"
    @update:model-value="setJsonValue"
  />
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { JsonRecord, RuninatorField, RuninatorType } from "../../types/models";
import { pretty } from "../../utils/format";
import JsonEditor from "./JsonEditor.vue";

defineOptions({ name: "TypedValueEditor" });

const props = defineProps<{
  modelValue: unknown;
  ty: RuninatorType;
  placeholder?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: unknown];
}>();

const typeKind = computed(() => props.ty?.type ?? "any");
const stringValue = computed(() => (typeof props.modelValue === "string" ? props.modelValue : ""));
const numberValue = computed(() => (typeof props.modelValue === "number" ? props.modelValue : ""));
const arrayValue = computed<unknown[]>(() => (Array.isArray(props.modelValue) ? props.modelValue : []));
const recordValue = computed<JsonRecord>(() => (isPlainRecord(props.modelValue) ? props.modelValue : {}));
const recordEntries = computed(() => Object.entries(recordValue.value));
const arrayItemType = computed(() => (props.ty.type === "array" ? props.ty.items : null));
const mapValueType = computed(() => (props.ty.type === "map" ? props.ty.values : null));
const structEntries = computed<Array<[string, RuninatorField]>>(() => (props.ty.type === "struct" ? Object.entries(props.ty.fields) : []));
const structFieldNames = computed(() => new Set(structEntries.value.map(([name]) => name)));
const structAdditionalType = computed(() => (props.ty.type === "struct" ? props.ty.additional ?? null : null));
const additionalStructEntries = computed(() => recordEntries.value.filter(([key]) => !structFieldNames.value.has(key)));
const unionVariants = computed(() => (props.ty.type === "union" ? props.ty.variants : []));
const unionVariantIndex = computed(() => selectedUnionVariantIndex(props.modelValue, unionVariants.value));
const selectedUnionVariant = computed(() => unionVariants.value[unionVariantIndex.value] ?? { type: "any" });
const unionValue = computed(() => matchesType(props.modelValue, selectedUnionVariant.value) ? props.modelValue : defaultValueForType(selectedUnionVariant.value));
const isStringArray = computed(() => props.ty.type === "array" && props.ty.items.type === "string");
const stringArrayText = computed(() => arrayValue.value.join("\n"));
const jsonText = computed(() => pretty(props.modelValue ?? defaultValueForType(props.ty)));

function emitValue(value: unknown) {
  emit("update:modelValue", value);
}

function setNumberValue(raw: string) {
  if (raw.trim() === "") {
    emitValue(null);
    return;
  }
  emitValue(typeKind.value === "integer" ? Number.parseInt(raw, 10) : Number(raw));
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
  if (!arrayItemType.value) return;
  emitValue([...arrayValue.value, defaultValueForType(arrayItemType.value)]);
}

function removeArrayItem(index: number) {
  emitValue(arrayValue.value.filter((_, itemIndex) => itemIndex !== index));
}

function setRecordField(key: string, value: unknown) {
  emitValue({ ...recordValue.value, [key]: value });
}

function renameRecordField(previousKey: string, nextKey: string) {
  if (previousKey === nextKey) return;
  const next = { ...recordValue.value };
  const value = next[previousKey];
  delete next[previousKey];
  next[nextKey] = value;
  emitValue(next);
}

function removeRecordField(key: string) {
  const next = { ...recordValue.value };
  delete next[key];
  emitValue(next);
}

function addRecordField() {
  const valueType = mapValueType.value ?? structAdditionalType.value;
  if (!valueType) return;
  const key = uniqueRecordKey(recordValue.value);
  setRecordField(key, defaultValueForType(valueType));
}

function selectUnionVariant(index: number) {
  const variant = unionVariants.value[index];
  if (!variant) return;
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
    key = `key_${index}`;
  }
  return key;
}

function selectedUnionVariantIndex(value: unknown, variants: RuninatorType[]): number {
  const match = variants.findIndex((variant) => matchesType(value, variant));
  return match >= 0 ? match : 0;
}

function matchesType(value: unknown, ty: RuninatorType): boolean {
  if (ty.type === "any") return true;
  if (ty.type === "null") return value === null;
  if (ty.type === "string") return typeof value === "string";
  if (ty.type === "boolean") return typeof value === "boolean";
  if (ty.type === "integer") return typeof value === "number" && Number.isInteger(value);
  if (ty.type === "number") return typeof value === "number" && !Number.isNaN(value);
  if (ty.type === "array") return Array.isArray(value);
  if (ty.type === "map" || ty.type === "struct") return isPlainRecord(value);
  if (ty.type === "union") return ty.variants.some((variant) => matchesType(value, variant));
  return false;
}

function defaultValueForType(ty: RuninatorType): unknown {
  if (ty.type === "string") return "";
  if (ty.type === "boolean") return false;
  if (ty.type === "integer" || ty.type === "number") return 0;
  if (ty.type === "array") return [];
  if (ty.type === "map" || ty.type === "struct") return {};
  if (ty.type === "union") return defaultValueForType(ty.variants[0] ?? { type: "any" });
  return null;
}

function describeType(ty: RuninatorType | undefined, depth = 0): string {
  if (!ty) return "any";
  if (ty.type === "array") return `${describeType(ty.items, depth + 1)}[]`;
  if (ty.type === "map") return `map<string, ${describeType(ty.values, depth + 1)}>`;
  if (ty.type === "union") return ty.variants.map((variant) => describeType(variant, depth + 1)).join(" | ");
  if (ty.type !== "struct") return ty.type;
  const entries = Object.entries(ty.fields);
  if (depth > 0 || entries.length > 3) return "struct";
  const fields = entries
    .map(([name, field]) => `${name}${field.required ? "" : "?"}: ${describeType(field.ty, depth + 1)}`)
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
.union-editor {
  display: grid;
  gap: 8px;
}

.typed-value-editor {
  min-width: 0;
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
  color: #66717e;
  font-size: 12px;
}

@media (max-width: 760px) {
  .collection-row,
  .array-editor .collection-row {
    grid-template-columns: 1fr;
  }
}
</style>
