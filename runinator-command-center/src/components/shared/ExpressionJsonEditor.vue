<template>
  <section class="expression-editor-shell">
    <header class="expression-editor-title">{{ title }}</header>
    <div ref="editorContainer" class="expression-editor-container"></div>
    <details class="expression-preview">
      <summary>Lowered value</summary>
      <pre>{{ loweredJson }}</pre>
    </details>
    <p v-if="parseError" class="expression-error">{{ parseError }}</p>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { completionKeymap, startCompletion } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { keymap, type ViewUpdate } from "@codemirror/view";
import { EditorView, basicSetup } from "codemirror";
import type { WorkflowExpressionEditorContext } from "../../utils/workflow-expression-completion";
import { workflowExpressionCompletionSource } from "../../utils/workflow-expression-completion";
import { wdl } from "../../utils/codemirror-lang-wdl";
import { pretty } from "../../utils/format";
import { expressionJsonToWdl, parseWdlExpression } from "../../utils/wdl-expression";

const props = defineProps<{
  modelValue: string;
  readonly?: boolean;
  title?: string;
  context?: WorkflowExpressionEditorContext;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const editorContainer = ref<HTMLElement | null>(null);
const parseError = ref("");
let view: EditorView | null = null;
const title = props.title ?? "WDL Expression";
const loweredJson = computed(() => props.modelValue);

onMounted(() => {
  if (!editorContainer.value) return;

  const startState = EditorState.create({
    doc: wdlFromLoweredJson(props.modelValue),
    extensions: [
      basicSetup,
      wdl(workflowExpressionCompletionSource(() => props.context)),
      keymap.of(completionKeymap),
      EditorView.editable.of(!props.readonly),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) updateLoweredJson(update.state.doc.toString());
        if (!props.readonly && shouldStartCompletion(update)) startCompletion(update.view);
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" }
      })
    ]
  });

  view = new EditorView({
    state: startState,
    parent: editorContainer.value
  });
});

watch(() => props.modelValue, (newValue) => {
  const nextWdl = wdlFromLoweredJson(newValue);
  if (view && nextWdl !== view.state.doc.toString()) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: nextWdl }
    });
  }
});

onBeforeUnmount(() => {
  view?.destroy();
});

function shouldStartCompletion(update: ViewUpdate): boolean {
  if (!update.docChanged) return false;
  const head = update.state.selection.main.head;
  if (head <= 0) return false;
  const previous = update.state.sliceDoc(head - 1, head);
  return /[\w.]/.test(previous);
}

function wdlFromLoweredJson(value: string): string {
  try {
    return expressionJsonToWdl(JSON.parse(value || "null"));
  } catch {
    return "null";
  }
}

function updateLoweredJson(source: string) {
  try {
    const lowered = parseWdlExpression(source);
    parseError.value = "";
    emit("update:modelValue", pretty(lowered));
  } catch (err) {
    parseError.value = err instanceof Error ? err.message : String(err);
  }
}
</script>

<style scoped>
.expression-editor-shell {
  display: flex;
  min-height: 164px;
  min-width: 0;
  flex-direction: column;
  border: 1px solid #ccd4dd;
  border-radius: 6px;
  background-color: #fff;
  overflow: hidden;
}

.expression-editor-title {
  padding: 7px 9px;
  color: #3b4652;
  font-size: 12px;
  font-weight: 700;
  user-select: none;
}

.expression-editor-container {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  border-top: 1px solid #e3e8ee;
  overflow: hidden;
}

.expression-preview {
  border-top: 1px solid #e3e8ee;
  background: #fbfcfe;
}

.expression-preview summary {
  cursor: pointer;
  padding: 6px 9px;
  color: #66717e;
  font-size: 12px;
  font-weight: 700;
}

.expression-preview pre {
  max-height: 160px;
  margin: 0;
  overflow: auto;
  padding: 8px 10px;
  border-top: 1px solid #e3e8ee;
  color: #2f3a45;
  font-size: 12px;
  white-space: pre-wrap;
}

.expression-error {
  margin: 0;
  padding: 7px 9px;
  border-top: 1px solid #f2c4c4;
  background: #fff5f5;
  color: #c53030;
  font-size: 12px;
}

:deep(.cm-editor) {
  height: 100%;
  min-height: 0;
}

:deep(.cm-scroller) {
  overflow: auto;
}
</style>
