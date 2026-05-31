<template>
  <details class="wdl-editor-shell" open>
    <summary>
      <span>{{ title }}</span>
      <button type="button" :disabled="readonly" @click.stop.prevent="formatDocument">Format</button>
    </summary>
    <div ref="editorContainer" class="wdl-editor-container"></div>
  </details>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { EditorState } from '@codemirror/state';
import { linter, type Diagnostic } from '@codemirror/lint';
import { wdl } from '../../utils/codemirror-lang-wdl';
import { analyzeWdl, formatWdl } from '../../api/commandCenterApi';
import { useAppStore } from '../../stores/app';

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
const app = useAppStore();

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

async function formatDocument() {
  if (!view || props.readonly) return;
  const source = view.state.doc.toString();
  let formatted: string;
  try {
    formatted = await formatWdl(source);
  } catch (err) {
    app.setError(`WDL format error: ${String(err)}`);
    return;
  }
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: formatted }
  });
  emit('update:modelValue', formatted);
}

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
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.wdl-editor-shell summary button {
  border: 1px solid #b8c3cf;
  border-radius: 4px;
  background: #f7f9fb;
  color: #1c2938;
  cursor: pointer;
  font: inherit;
  font-weight: 600;
  padding: 3px 8px;
}

.wdl-editor-shell summary button:disabled {
  cursor: not-allowed;
  opacity: 0.5;
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
