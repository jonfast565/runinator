<template>
  <section class="expression-editor-shell" @mousedown.stop @click.stop>
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
      <pre v-if="previewUnresolved" class="expression-preview-muted">{{ previewUnresolved }}</pre>
      <pre v-else-if="previewError" class="expression-preview-error">{{ previewError }}</pre>
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
import { EditorState, Prec } from "@codemirror/state";
import { keymap, type ViewUpdate } from "@codemirror/view";
import { EditorView, basicSetup } from "codemirror";
import type { WorkflowExpressionEditorContext } from "../../../utils/workflow-expression-completion";
import { workflowExpressionCompletionSource } from "../../../utils/workflow-expression-completion";
import { workflowReferenceGroups } from "../../../utils/workflow-references";
import {
  clearExpressionInsertTarget,
  setExpressionInsertTarget,
} from "../../../utils/expression-insert-target";
import { wdl } from "../../../utils/codemirror-lang-wdl";
import { osCodeMirrorTheme } from "../../../utils/codemirror-theme";
import { pretty } from "../../../utils/format";
import { expressionJsonToWdl, parseWdlExpression } from "../../../utils/wdl-expression";
import { expressionService } from "../../../core/services";
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
// a reference that resolves in the full workflow run but is absent from this preview's sample is
// not an authoring error; it is surfaced as a muted note rather than a red error.
const previewUnresolved = ref("");
let view: EditorView | null = null;
let disposeEditorTheme: (() => void) | null = null;
let previewTimer: ReturnType<typeof setTimeout> | null = null;
let previewToken = 0;
// the last value this editor emitted; used to ignore the parent echoing it straight back, so an
// edit in progress is not reformatted by the json<->wdl round-trip moving the cursor.
let lastEmitted: string | null = null;
const title = computed(() => props.title ?? "WDL Expression");
const loweredJson = computed(() => props.modelValue);
const referenceGroups = computed(() => workflowReferenceGroups(props.context));
// a preview is only meaningful when a prior run's data is available to resolve against.
const hasSample = computed(() => Boolean(props.context?.sampleContext));

// resolve the current lowered expression against the sample context, debounced so it does not fire
// on every keystroke. a resolver error (e.g. a config/secret ref absent in the preview) is shown in
// place rather than thrown.
function schedulePreview() {
  if (previewTimer) {
    clearTimeout(previewTimer);
  }

  if (!hasSample.value) {
    previewResult.value = "";
    previewError.value = "";
    previewUnresolved.value = "";
    return;
  }

  previewTimer = setTimeout(runPreview, 250);
}

async function runPreview() {
  const sample = props.context?.sampleContext;

  if (!sample) {
    return;
  }

  let expression: unknown;

  try {
    expression = JSON.parse(props.modelValue || "null");
  } catch {
    return; // mid-edit invalid json; keep the last good preview.
  }

  const token = ++previewToken;

  try {
    const resolved = await expressionService.evaluateSilent(expression, sample);

    if (token !== previewToken) {
      return;
    } // a newer edit superseded this request.

    previewError.value = "";
    previewUnresolved.value = "";
    previewResult.value = pretty(resolved);
  } catch (err) {
    if (token !== previewToken) {
      return;
    }

    const message = err instanceof Error ? err.message : String(err);
    previewResult.value = "";

    // an unresolved reference (WORKFLOW017) is expected for refs not captured in the sample run.
    if (isUnresolvedReferenceError(message)) {
      previewError.value = "";
      previewUnresolved.value = "Not available in this preview (resolves at runtime).";
      return;
    }

    previewError.value = message;
    previewUnresolved.value = "";
  }
}

// splice a reference at the cursor, replacing any selection, then return focus to the editor.
function insertReference(text: string) {
  if (!view) {
    return;
  }

  view.dispatch(view.state.replaceSelection(text));
  view.focus();
}

// wrap the current selection in a transform, leaving the cursor after the inserted text.
function applyTransform(kind: "string" | "json" | "coalesce" | "concat") {
  if (!view) {
    return;
  }

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
  if (!editorContainer.value) {
    return;
  }

  const editorTheme = osCodeMirrorTheme();

  const startState = EditorState.create({
    doc: wdlFromLoweredJson(props.modelValue),
    extensions: [
      basicSetup,
      editorTheme.extension,
      wdl(workflowExpressionCompletionSource(() => props.context)),
      Prec.high(
        keymap.of([
          ...completionKeymap,
          {
            key: "Tab",
            run(editor) {
              if (props.readonly) {
                return false;
              }

              editor.dispatch(editor.state.replaceSelection("    "));
              return true;
            },
          },
        ]),
      ),
      EditorView.editable.of(!props.readonly),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          updateLoweredJson(update.state.doc.toString());
        }

        if (!props.readonly && shouldStartCompletion(update)) {
          startCompletion(update.view);
        }

        // claim/release the dialog-level insert slot so the reference chips target this field.
        if (update.focusChanged && !props.readonly) {
          if (update.view.hasFocus) {
            setExpressionInsertTarget(insertReference);
          } else {
            clearExpressionInsertTarget(insertReference);
          }
        }
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
      }),
    ],
  });

  view = new EditorView({
    state: startState,
    parent: editorContainer.value,
  });
  disposeEditorTheme = editorTheme.install(view);

  schedulePreview();
});

watch(
  () => props.modelValue,
  (newValue) => {
    // ignore the parent echoing back exactly what we just emitted; re-deriving the wdl text would
    // reformat the doc and jump the cursor mid-edit.
    if (newValue === lastEmitted) {
      lastEmitted = null;
      schedulePreview();
      return;
    }

    const nextWdl = wdlFromLoweredJson(newValue);

    if (view && nextWdl !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: nextWdl },
      });
    }

    schedulePreview();
  },
);

// re-resolve when a run's data becomes available or changes while the editor is open.
watch(() => props.context?.sampleContext, schedulePreview);

onBeforeUnmount(() => {
  if (previewTimer) {
    clearTimeout(previewTimer);
  }

  disposeEditorTheme?.();
  clearExpressionInsertTarget(insertReference);
  view?.destroy();
});

function shouldStartCompletion(update: ViewUpdate): boolean {
  if (!update.docChanged) {
    return false;
  }

  if (!update.transactions.some((transaction) => transaction.isUserEvent("input"))) {
    return false;
  }

  const head = update.state.selection.main.head;

  if (head <= 0) {
    return false;
  }

  const previous = update.state.sliceDoc(head - 1, head);
  return /[\w.]/.test(previous);
}

// the backend tags an invalid/absent runtime value reference with the WORKFLOW017 code.
function isUnresolvedReferenceError(message: string): boolean {
  return /WORKFLOW0?17\b/i.test(message);
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
    const next = pretty(lowered);
    lastEmitted = next;
    emit("update:modelValue", next);
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
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background-color: var(--surface);
  overflow: hidden;
}

.expression-editor-title {
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

.expression-picker-toggle {
  padding: 2px 9px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text-subtle);
  cursor: pointer;
  font-size: 11px;
  font-weight: 700;
}

.expression-picker-toggle:hover,
.expression-picker-toggle.active {
  background: var(--surface-hover);
  border-color: var(--border-hover);
}

.expression-editor-container {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  border-top: 1px solid var(--border-subtle);
  overflow: hidden;
}

.expression-preview {
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
}

.expression-preview summary {
  cursor: pointer;
  padding: 6px 9px;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 700;
}

.expression-preview pre {
  max-height: 160px;
  margin: 0;
  overflow: auto;
  padding: 8px 10px;
  border-top: 1px solid var(--border-subtle);
  color: var(--text);
  font-size: 12px;
  white-space: pre-wrap;
}

.expression-preview pre.expression-preview-error {
  color: var(--warning-fg);
  background: var(--warning-bg);
}

.expression-preview pre.expression-preview-muted {
  color: var(--text-faint);
  font-style: italic;
}

.expression-error {
  margin: 0;
  padding: 7px 9px;
  border-top: 1px solid var(--danger-bg);
  background: var(--danger-bg);
  color: var(--danger-fg);
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
