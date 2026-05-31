<template>
  <details class="wdl-editor-shell" open>
    <summary>{{ title }}</summary>
    <div ref="editorContainer" class="wdl-editor-container"></div>
  </details>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { EditorState } from '@codemirror/state';
import { linter, type Diagnostic } from '@codemirror/lint';
import { wdl } from '../../utils/codemirror-lang-wdl';
import { analyzeWdl } from '../../api/commandCenterApi';

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
const title = props.title ?? "WDL";

// async linter backed by the rust runinator-wdl compiler, so editor diagnostics match
// what the importer would report. codemirror debounces this by default.
const wdlLinter = linter(async (linterView): Promise<Diagnostic[]> => {
  const source = linterView.state.doc.toString();
  const docLength = linterView.state.doc.length;
  let diagnostics;
  try {
    diagnostics = await analyzeWdl(source);
  } catch {
    return [];
  }
  return diagnostics.map((diagnostic) => {
    const from = Math.min(Math.max(diagnostic.start, 0), docLength);
    let to = Math.min(Math.max(diagnostic.end, from), docLength);
    if (to <= from) {
      to = Math.min(from + 1, docLength);
    }
    return {
      from,
      to,
      severity: diagnostic.severity === "warning" ? "warning" : "error",
      message: diagnostic.message,
    };
  });
});

onMounted(() => {
  if (!editorContainer.value) return;

  const startState = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      wdl(),
      wdlLinter,
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
.wdl-editor-shell {
  border: 1px solid #ccd4dd;
  border-radius: 6px;
  background-color: #fff;
  overflow: hidden;
}

.wdl-editor-shell summary {
  cursor: pointer;
  padding: 8px 10px;
  font-weight: 600;
  user-select: none;
}

.wdl-editor-container {
  height: 220px;
  width: 100%;
  border-top: 1px solid #e3e8ee;
  overflow: hidden;
}

:deep(.cm-editor) {
  height: 100%;
}
</style>
