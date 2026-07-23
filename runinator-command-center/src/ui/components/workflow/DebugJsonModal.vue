<template>
  <Modal :title="title" width="min(640px, 90vw)" @close="$emit('close')">
    <p v-if="hint" class="hint m-0">{{ hint }}</p>
    <div class="min-h-0 flex-1 [&_.json-editor-container]:h-[280px]">
      <JsonEditor :model-value="text" :title="editorTitle" @update:model-value="onChange" />
    </div>
    <div v-if="error" class="error text-xs">{{ error }}</div>
    <template #actions>
      <button class="btn" @click="$emit('close')">Cancel</button>
      <button class="btn btn-primary" :disabled="!isValid" @click="onSubmit">{{ submitLabel }}</button>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import JsonEditor from "../shared/JsonEditor.vue";
import Modal from "../shared/Modal.vue";

const props = defineProps<{
  title: string;
  hint?: string;
  initialValue: unknown;
  editorTitle?: string;
  submitLabel?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [value: unknown];
}>();

const text = ref(format(props.initialValue));
const error = ref<string>("");

watch(
  () => props.initialValue,
  (next) => {
    text.value = format(next);
  },
);

const submitLabel = computed(() => props.submitLabel ?? "Submit");
const editorTitle = computed(() => props.editorTitle ?? "JSON");

const isValid = computed(() => {
  try {
    JSON.parse(text.value);
    return true;
  } catch {
    return false;
  }
});

function format(value: unknown): string {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch {
    return "{}";
  }
}

function onChange(next: string) {
  text.value = next;
  error.value = "";
}

function onSubmit() {
  try {
    const parsed: unknown = JSON.parse(text.value);
    emit("submit", parsed);
  } catch (err) {
    error.value = err instanceof Error ? err.message : "Invalid JSON";
  }
}
</script>
