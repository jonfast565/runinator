<template>
  <details class="json-editor-shell" open>
    <summary>{{ title }}</summary>
    <div ref="editorContainer" class="json-editor-container"></div>
  </details>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { json } from '@codemirror/lang-json';
import { EditorState } from '@codemirror/state';

const props = defineProps<{ 
  modelValue: string;
  readonly?: boolean;
  title?: string;
}>();

const emit = defineEmits<{ 
  "update:modelValue": [value: string] 
}>();

const editorContainer = ref<HTMLElement | null>(null);
let view: EditorView | null = null;
const title = props.title ?? "JSON";

onMounted(() => {
  if (!editorContainer.value) return;

  const startState = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      json(),
      EditorView.lineWrapping,
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
  border: 1px solid #ccd4dd;
  border-radius: 6px;
  background-color: #fff;
  overflow: hidden;
}

.json-editor-shell summary {
  cursor: pointer;
  padding: 8px 10px;
  font-weight: 600;
  user-select: none;
}

.json-editor-container {
  height: 220px;
  width: 100%;
  border-top: 1px solid #e3e8ee;
  overflow: hidden;
}

:deep(.cm-editor) {
  height: 100%;
}
</style>
