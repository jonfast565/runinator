<template>
  <section class="json-editor-shell" @mousedown.stop @click.stop>
    <header v-if="title" class="json-editor-title">
      <span>{{ title }}</span>
      <button
        type="button"
        class="json-editor-copy"
        :title="copied ? 'Copied' : 'Copy JSON'"
        @click="copy"
      >
        {{ copied ? "Copied" : "Copy" }}
      </button>
    </header>
    <div ref="editorContainer" class="json-editor-container"></div>
    <p v-if="!readonly && parseError" class="json-editor-error" role="alert">
      <Icon name="alert" :size="12" />
      <span>{{ parseError }}</span>
    </p>
  </section>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from "vue";
import { getTextEditorHostFactory } from "../../../core/platform";
import type { CodeMirrorHostOptions } from "../../adapters/codemirror/text-editor-host";
import Icon from "./Icon.vue";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    readonly?: boolean;
    keyHints?: string[];
    title?: string;
  }>(),
  { title: "JSON", keyHints: undefined },
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const editorContainer = ref<HTMLElement | null>(null);
const title = props.title;
const copied = ref(false);
const parseError = ref("");
let host: ReturnType<ReturnType<typeof getTextEditorHostFactory>["create"]> | null = null;

function validate(text: string) {
  const trimmed = text.trim();

  if (!trimmed) {
    parseError.value = "";
    return;
  }

  try {
    JSON.parse(trimmed);
    parseError.value = "";
  } catch (err) {
    parseError.value = err instanceof Error ? err.message : "Invalid JSON";
  }
}

async function copy() {
  try {
    await navigator.clipboard.writeText(props.modelValue);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1200);
  } catch {
    // clipboard may be unavailable; ignore.
  }
}

onMounted(() => {
  if (!editorContainer.value) {
    return;
  }

  const options: CodeMirrorHostOptions = {
    language: "json",
    value: props.modelValue,
    readonly: props.readonly,
    jsonKeyHints: () => props.keyHints ?? [],
    onChange(text) {
      emit("update:modelValue", text);

      if (!props.readonly) {
        validate(text);
      }
    },
  };

  host = getTextEditorHostFactory().create(options);
  host.mount(editorContainer.value);

  if (!props.readonly) {
    validate(props.modelValue);
  }
});

watch(
  () => props.modelValue,
  (newValue) => {
    host?.setValue(newValue, true);

    if (!props.readonly) {
      validate(newValue);
    }
  },
);

watch(
  () => props.readonly,
  (readonly) => {
    host?.setReadonly(Boolean(readonly));
  },
);

onBeforeUnmount(() => {
  host?.destroy();
  host = null;
});
</script>

<style scoped>
.json-editor-shell {
  display: flex;
  flex: 1 1 auto;
  height: auto;
  min-height: 220px;
  min-width: 0;
  flex-direction: column;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background-color: var(--surface);
  overflow: hidden;
}

.json-editor-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 9px;
  color: var(--text-subtle);
  font-size: 12px;
  font-weight: 700;
  user-select: none;
}

.json-editor-copy {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text-muted);
  cursor: pointer;
  font: inherit;
  font-size: 11px;
  font-weight: 600;
  padding: 2px 8px;
}

.json-editor-copy:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.json-editor-container {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  border-top: 1px solid var(--border-subtle);
  overflow: hidden;
}

.json-editor-error {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: 5px 9px;
  border-top: 1px solid var(--danger-bg);
  background: var(--danger-bg);
  color: var(--danger-fg);
  font-size: 11px;
}

:deep(.cm-editor) {
  height: 100%;
  min-height: 0;
}

:deep(.cm-scroller) {
  overflow: auto;
}
</style>
