<template>
  <div class="run-input-form">
    <div class="rif-toolbar">
      <div class="rif-modes">
        <button type="button" :class="{ active: mode === 'form' }" @click="setMode('form')">Form</button>
        <button type="button" :class="{ active: mode === 'json' }" @click="setMode('json')">Raw JSON</button>
      </div>
      <div class="rif-presets">
        <select v-model="selectedPreset" @change="applyPreset">
          <option value="">Presets…</option>
          <option v-for="preset in presets" :key="preset.name" :value="preset.name">{{ preset.name }}</option>
        </select>
        <button type="button" :disabled="!hasLast" title="Load the input from the last run" @click="useLast">Last input</button>
        <button type="button" title="Save the current input as a preset" @click="savePreset">Save</button>
        <button v-if="selectedPreset" type="button" class="rif-danger" title="Delete the selected preset" @click="deletePreset">Delete</button>
      </div>
    </div>

    <div v-if="mode === 'form'" class="rif-body">
      <TypedValueEditor
        :model-value="modelValue"
        :ty="orderedInputType"
        :allow-expressions="false"
        @update:model-value="emitValue"
      />
    </div>
    <div v-else class="rif-json">
      <textarea v-model="jsonDraft" class="rif-json-input" spellcheck="false" @input="onJsonInput"></textarea>
      <div v-if="jsonError" class="rif-json-error">{{ jsonError }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import TypedValueEditor from "./TypedValueEditor.vue";
import { pretty } from "../../utils/format";
import type { RuninatorType } from "../../types/models";

interface RunInputPreset {
  name: string;
  value: unknown;
}

const props = defineProps<{
  modelValue: unknown;
  inputType: RuninatorType;
  // namespaces preset / last-input storage, e.g. a workflow id or name.
  storageKey: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: unknown];
}>();

const mode = ref<"form" | "json">("form");
const jsonDraft = ref(pretty(props.modelValue ?? {}));
const jsonError = ref("");
const presets = ref<RunInputPreset[]>([]);
const selectedPreset = ref("");
const hasLast = ref(false);

// render required struct fields first so the form reads top-to-bottom by importance.
const orderedInputType = computed<RuninatorType>(() => {
  const ty = props.inputType;
  if (ty?.type !== "struct") return ty ?? { type: "any" };
  const entries = Object.entries(ty.fields);
  const ordered = [...entries].sort(([, left], [, right]) => Number(right.required) - Number(left.required));
  return { ...ty, fields: Object.fromEntries(ordered) };
});

function presetsStorageKey() {
  return `runinator.runInput.presets.${props.storageKey}`;
}
function lastStorageKey() {
  return `runinator.runInput.last.${props.storageKey}`;
}

function loadPresets() {
  try {
    const raw = window.localStorage.getItem(presetsStorageKey());
    presets.value = raw ? (JSON.parse(raw) as RunInputPreset[]) : [];
  } catch {
    presets.value = [];
  }
  hasLast.value = window.localStorage.getItem(lastStorageKey()) != null;
}

function emitValue(value: unknown) {
  emit("update:modelValue", value);
  if (mode.value === "json") jsonDraft.value = pretty(value ?? {});
}

function setMode(next: "form" | "json") {
  if (next === mode.value) return;
  if (next === "json") {
    jsonDraft.value = pretty(props.modelValue ?? {});
    jsonError.value = "";
  }
  mode.value = next;
}

function onJsonInput() {
  try {
    const parsed = JSON.parse(jsonDraft.value || "null");
    jsonError.value = "";
    emit("update:modelValue", parsed);
  } catch (err) {
    jsonError.value = String(err);
  }
}

function applyPreset() {
  if (!selectedPreset.value) return;
  const preset = presets.value.find((item) => item.name === selectedPreset.value);
  if (!preset) return;
  emitValue(structuredClone(preset.value));
}

function savePreset() {
  const name = window.prompt("Preset name")?.trim();
  if (!name) return;
  const next = presets.value.filter((item) => item.name !== name);
  next.push({ name, value: structuredClone(toRawValue(props.modelValue)) });
  next.sort((left, right) => left.name.localeCompare(right.name));
  presets.value = next;
  window.localStorage.setItem(presetsStorageKey(), JSON.stringify(next));
  selectedPreset.value = name;
}

function deletePreset() {
  if (!selectedPreset.value) return;
  presets.value = presets.value.filter((item) => item.name !== selectedPreset.value);
  window.localStorage.setItem(presetsStorageKey(), JSON.stringify(presets.value));
  selectedPreset.value = "";
}

function useLast() {
  try {
    const raw = window.localStorage.getItem(lastStorageKey());
    if (raw == null) return;
    emitValue(JSON.parse(raw));
    mode.value = "form";
  } catch {
    // a corrupt last-input entry is ignored rather than blocking the run.
  }
}

// strip vue reactivity so stored json is plain.
function toRawValue(value: unknown): unknown {
  return JSON.parse(JSON.stringify(value ?? {}));
}

// persist the current input as the "last run" input; the parent calls this when a run starts.
function persistLast() {
  try {
    window.localStorage.setItem(lastStorageKey(), JSON.stringify(toRawValue(props.modelValue)));
    hasLast.value = true;
  } catch {
    // storage may be unavailable; persistence is best-effort.
  }
}

defineExpose({ persistLast });

// switching the target workflow reloads its presets and resets the editor mode.
watch(
  () => props.storageKey,
  () => {
    selectedPreset.value = "";
    jsonError.value = "";
    mode.value = "form";
    loadPresets();
  },
  { immediate: true }
);
</script>

<style scoped>
.run-input-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.rif-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.rif-modes button,
.rif-presets button {
  border: 1px solid #c8d1db;
  border-radius: 4px;
  background: #f8fafc;
  color: #4b5663;
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  padding: 4px 8px;
}
.rif-modes {
  display: inline-flex;
  gap: 4px;
}
.rif-modes button.active {
  border-color: #2f3a45;
  background: #2f3a45;
  color: #fff;
}
.rif-presets {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.rif-presets select {
  font-size: 12px;
  padding: 3px 6px;
}
.rif-presets button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.rif-danger {
  color: #b91c1c;
  border-color: #f3c2c2 !important;
}
.rif-body {
  min-width: 0;
}
.rif-json-input {
  width: 100%;
  min-height: 120px;
  resize: vertical;
  font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
.rif-json-error {
  margin-top: 4px;
  color: #b91c1c;
  font-size: 11px;
}
</style>
