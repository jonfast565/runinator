<template>
  <div class="modal-backdrop" @click.self="$emit('close')">
    <div class="modal-card">
      <header class="modal-header">
        <h3>{{ title }}</h3>
        <button class="modal-close" @click="$emit('close')">×</button>
      </header>
      <p v-if="hint" class="modal-hint">{{ hint }}</p>
      <div class="modal-editor">
        <JsonEditor :model-value="text" :title="editorTitle" @update:model-value="onChange" />
      </div>
      <div v-if="error" class="modal-error">{{ error }}</div>
      <footer class="modal-footer">
        <button @click="$emit('close')">Cancel</button>
        <button class="primary" :disabled="!isValid" @click="onSubmit">{{ submitLabel }}</button>
      </footer>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import JsonEditor from "../shared/JsonEditor.vue";

const props = defineProps<{
  title: string;
  hint?: string;
  initialValue: any;
  editorTitle?: string;
  submitLabel?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [value: any];
}>();

const text = ref(format(props.initialValue));
const error = ref<string>("");

watch(() => props.initialValue, (next) => {
  text.value = format(next);
});

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

function format(value: any): string {
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
    const parsed = JSON.parse(text.value);
    emit("submit", parsed);
  } catch (err) {
    error.value = err instanceof Error ? err.message : "Invalid JSON";
  }
}
</script>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(15, 23, 42, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.modal-card {
  width: min(640px, 90vw);
  max-height: 80vh;
  background: #fff;
  border-radius: 8px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-shadow: 0 12px 32px rgba(15, 23, 42, 0.25);
}
.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.modal-header h3 {
  margin: 0;
}
.modal-close {
  background: transparent;
  border: 0;
  font-size: 20px;
  cursor: pointer;
  color: #64748b;
}
.modal-hint {
  font-size: 12px;
  color: #64748b;
  margin: 0;
}
.modal-editor {
  flex: 1;
  min-height: 0;
}
.modal-editor :deep(.json-editor-container) {
  height: 280px;
}
.modal-error {
  color: #dc2626;
  font-size: 12px;
}
.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.modal-footer button.primary {
  background: #2563eb;
  border-color: #2563eb;
  color: #fff;
}
.modal-footer button.primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
