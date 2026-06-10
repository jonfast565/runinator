<template>
  <section class="json-editor-shell" @mousedown.stop @click.stop>
    <header v-if="title" class="json-editor-title">
      <span>{{ title }}</span>
      <button type="button" class="json-editor-copy" :title="copied ? 'Copied' : 'Copy JSON'" @click="copy">
        {{ copied ? "Copied" : "Copy" }}
      </button>
    </header>
    <div ref="editorContainer" class="json-editor-container"></div>
  </section>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { json } from '@codemirror/lang-json';
import { EditorState } from '@codemirror/state';

const props = withDefaults(defineProps<{
  modelValue: string;
  readonly?: boolean;
  // header label; pass an empty string to hide the title bar.
  title?: string;
}>(), { title: "JSON" });

const emit = defineEmits<{
  "update:modelValue": [value: string]
}>();

const editorContainer = ref<HTMLElement | null>(null);
let view: EditorView | null = null;
const title = props.title;
const copied = ref(false);

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
  if (!editorContainer.value) return;

  const startState = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      json(),
      EditorView.editable.of(!props.readonly),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          emit('update:modelValue', update.state.doc.toString());
        }
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" }
      })
    ],
  });

  view = new EditorView({
    state: startState,
    parent: editorContainer.value,
  });
});

watch(() => props.modelValue, (newValue) => {
  if (view && newValue !== view.state.doc.toString()) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: newValue }
    });
  }
});

onBeforeUnmount(() => {
  if (view) {
    view.destroy();
  }
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

:deep(.cm-editor) {
  height: 100%;
  min-height: 0;
}

:deep(.cm-scroller) {
  overflow: auto;
}
</style>
