<template>
  <section class="expression-editor-shell">
    <header class="expression-editor-title">
      <span>{{ title }}</span>
      <button
        v-if="!readonly"
        type="button"
        class="expression-picker-toggle"
        :class="{ active: showPicker }"
        @click="showPicker = !showPicker"
      >
        {{ showPicker ? "Hide references" : "Insert reference" }}
      </button>
    </header>
    <div ref="editorContainer" class="expression-editor-container"></div>
    <ReferencePicker
      v-if="showPicker && !readonly"
      :groups="referenceGroups"
      @insert="insertReference"
      @transform="applyTransform"
    />
    <details v-if="hasSample" class="expression-preview" open>
      <summary>Resolved against last run</summary>
      <pre v-if="previewError" class="expression-preview-error">{{ previewError }}</pre>
      <pre v-else>{{ previewResult }}</pre>
    </details>
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
import { workflowReferenceGroups } from "../../utils/workflow-references";
import { wdl } from "../../utils/codemirror-lang-wdl";
import { pretty } from "../../utils/format";
import { expressionJsonToWdl, parseWdlExpression } from "../../utils/wdl-expression";
import { evaluateExpression } from "../../api/commandCenterApi";
import ReferencePicker from "./ReferencePicker.vue";

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
const showPicker = ref(false);
const previewResult = ref("");
const previewError = ref("");
let view: EditorView | null = null;
let previewTimer: ReturnType<typeof setTimeout> | null = null;
let previewToken = 0;
const title = props.title ?? "WDL Expression";
const loweredJson = computed(() => props.modelValue);
const referenceGroups = computed(() => workflowReferenceGroups(props.context));
// a preview is only meaningful when a prior run's data is available to resolve against.
const hasSample = computed(() => Boolean(props.context?.sampleContext));

// resolve the current lowered expression against the sample context, debounced so it does not fire
// on every keystroke. a resolver error (e.g. a config/secret ref absent in the preview) is shown in
// place rather than thrown.
function schedulePreview() {
  if (previewTimer) clearTimeout(previewTimer);
  if (!hasSample.value) {
    previewResult.value = "";
    previewError.value = "";
    return;
  }
  previewTimer = setTimeout(runPreview, 250);
}

async function runPreview() {
  const sample = props.context?.sampleContext;
  if (!sample) return;
  let expression: unknown;
  try {
    expression = JSON.parse(props.modelValue || "null");
  } catch {
    return; // mid-edit invalid json; keep the last good preview.
  }
  const token = ++previewToken;
  try {
    const resolved = await evaluateExpression(expression, sample);
    if (token !== previewToken) return; // a newer edit superseded this request.
    previewError.value = "";
    previewResult.value = pretty(resolved);
  } catch (err) {
    if (token !== previewToken) return;
    previewError.value = err instanceof Error ? err.message : String(err);
    previewResult.value = "";
  }
}

// splice a reference at the cursor, replacing any selection, then return focus to the editor.
function insertReference(text: string) {
  if (!view) return;
  view.dispatch(view.state.replaceSelection(text));
  view.focus();
}

// wrap the current selection in a transform, leaving the cursor after the inserted text.
function applyTransform(kind: "string" | "json" | "coalesce" | "concat") {
  if (!view) return;
  const { from, to } = view.state.selection.main;
  const selected = view.state.sliceDoc(from, to);
  const insert =
    kind === "string"
      ? `string(${selected})`
      : kind === "json"
        ? `json(${selected})`
        : kind === "coalesce"
          ? `${selected} ?? `
          : `${selected} ++ `;
  view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length } });
  view.focus();
}

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

  schedulePreview();
});

watch(() => props.modelValue, (newValue) => {
  const nextWdl = wdlFromLoweredJson(newValue);
  if (view && nextWdl !== view.state.doc.toString()) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: nextWdl }
    });
  }
  schedulePreview();
});

// re-resolve when a run's data becomes available or changes while the editor is open.
watch(() => props.context?.sampleContext, schedulePreview);

onBeforeUnmount(() => {
  if (previewTimer) clearTimeout(previewTimer);
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
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 9px;
  color: #3b4652;
  font-size: 12px;
  font-weight: 700;
  user-select: none;
}

.expression-picker-toggle {
  padding: 2px 9px;
  border: 1px solid #ccd4dd;
  border-radius: 4px;
  background: #fff;
  color: #3b4652;
  cursor: pointer;
  font-size: 11px;
  font-weight: 700;
}

.expression-picker-toggle:hover,
.expression-picker-toggle.active {
  background: #eef3f9;
  border-color: #aeb9c4;
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

.expression-preview pre.expression-preview-error {
  color: #b05a16;
  background: #fffaf3;
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
