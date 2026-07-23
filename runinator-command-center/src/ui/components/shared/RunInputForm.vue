<template>
  <div class="run-input-form">
    <div class="rif-toolbar">
      <div class="rif-modes">
        <button type="button" :class="{ active: mode === 'form' }" @click="setMode('form')">
          Form
        </button>
        <button type="button" :class="{ active: mode === 'json' }" @click="setMode('json')">
          Raw JSON
        </button>
      </div>
      <div class="rif-presets">
        <select v-model="selectedPreset" @change="applyPreset">
          <option value="">Presets…</option>
          <option v-for="preset in presets" :key="preset.name" :value="preset.name">
            {{ preset.name }}
          </option>
        </select>
        <button
          type="button"
          :disabled="!hasLast"
          title="Load the input from the last run"
          @click="useLast"
        >
          Last input
        </button>
        <button type="button" title="Save the current input as a preset" @click="savePreset">
          Save
        </button>
        <button
          v-if="selectedPreset"
          type="button"
          class="rif-danger"
          title="Delete the selected preset"
          @click="deletePreset"
        >
          Delete
        </button>
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
      <JsonEditor
        class="rif-json-editor"
        :model-value="jsonDraft"
        :key-hints="jsonKeyHints"
        @update:model-value="onJsonInput"
      />
      <div v-if="jsonError" class="rif-json-error">{{ jsonError }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import JsonEditor from "./JsonEditor.vue";
import TypedValueEditor from "./TypedValueEditor.vue";
import { pretty } from "../../../core/utils/format";
import type { RuninatorType } from "../../../core/domain/models";

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

  if (ty.type !== "struct") {
    return ty;
  }

  const entries = Object.entries(ty.fields);
  const ordered = [...entries].sort(
    ([, left], [, right]) => Number(right.required) - Number(left.required),
  );
  return { ...ty, fields: Object.fromEntries(ordered) };
});

const jsonKeyHints = computed(() =>
  orderedInputType.value.type === "struct" ? Object.keys(orderedInputType.value.fields) : [],
);

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

  if (mode.value === "json") {
    jsonDraft.value = pretty(value ?? {});
  }
}

function setMode(next: "form" | "json") {
  if (next === mode.value) {
    return;
  }

  if (next === "json") {
    jsonDraft.value = pretty(props.modelValue ?? {});
    jsonError.value = "";
  }

  mode.value = next;
}

function onJsonInput(value: string) {
  jsonDraft.value = value;

  try {
    const parsed: unknown = JSON.parse(jsonDraft.value || "null");
    jsonError.value = "";
    emit("update:modelValue", parsed);
  } catch (err) {
    jsonError.value = String(err);
  }
}

function applyPreset() {
  if (!selectedPreset.value) {
    return;
  }

  const preset = presets.value.find((item) => item.name === selectedPreset.value);

  if (!preset) {
    return;
  }

  emitValue(structuredClone(preset.value));
}

function savePreset() {
  const name = window.prompt("Preset name")?.trim();

  if (!name) {
    return;
  }

  const next = presets.value.filter((item) => item.name !== name);
  next.push({ name, value: structuredClone(toRawValue(props.modelValue)) });
  next.sort((left, right) => left.name.localeCompare(right.name));
  presets.value = next;
  window.localStorage.setItem(presetsStorageKey(), JSON.stringify(next));
  selectedPreset.value = name;
}

function deletePreset() {
  if (!selectedPreset.value) {
    return;
  }

  presets.value = presets.value.filter((item) => item.name !== selectedPreset.value);
  window.localStorage.setItem(presetsStorageKey(), JSON.stringify(presets.value));
  selectedPreset.value = "";
}

function useLast() {
  try {
    const raw = window.localStorage.getItem(lastStorageKey());

    if (raw == null) {
      return;
    }

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
  { immediate: true },
);
</script>

