<template>
  <section class="wdl-editor-shell" @mousedown.stop @click.stop>
    <header class="wdl-editor-title">
      <span>{{ title }}</span>
      <div class="wdl-editor-actions">
        <span :class="['wdl-diagnostic-summary', diagnosticSummaryClass]">{{
          diagnosticSummary
        }}</span>
        <button type="button" :disabled="readonly" @click.stop.prevent="formatDocument">
          Format
        </button>
      </div>
    </header>
    <div ref="editorContainer" class="wdl-editor-container"></div>
    <div class="wdl-diagnostics">
      <table v-if="diagnostics.length">
        <thead>
          <tr>
            <th>Type</th>
            <th>What</th>
            <th>Line</th>
            <th>Col</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="diagnostic in diagnostics"
            :key="diagnosticKey(diagnostic)"
            :class="diagnostic.severity"
            @click="goToDiagnostic(diagnostic)"
          >
            <td>
              <span :class="['wdl-diagnostic-severity', diagnostic.severity]">{{
                diagnostic.severity
              }}</span>
            </td>
            <td>{{ diagnostic.message }}</td>
            <td>{{ diagnostic.line }}</td>
            <td>{{ diagnostic.column }}</td>
          </tr>
        </tbody>
      </table>
      <div v-else class="wdl-diagnostics-empty">No WDL diagnostics.</div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, watch, onBeforeUnmount } from "vue";
import { EditorView, basicSetup } from "codemirror";
import { completionKeymap, startCompletion } from "@codemirror/autocomplete";
import { Compartment, EditorState, Prec } from "@codemirror/state";
import { keymap, type ViewUpdate } from "@codemirror/view";
import { linter, type Diagnostic } from "@codemirror/lint";
import { wdl } from "../../../utils/codemirror-lang-wdl";
import { osCodeMirrorTheme } from "../../../utils/codemirror-theme";
import { wdlProviderCompletionSource } from "../../../utils/wdl-completion";
import { wdlHoverTooltip } from "../../../utils/wdl-hover";
import { analyzeWdl, formatWdl } from "../../../api/commandCenterApi";
import { useAppStore } from "../../../stores/app";
import type {
  CredentialSummary,
  ProviderMetadata,
  WdlDiagnostic,
  WdlSettingRef,
} from "../../../types/models";

const props = defineProps<{
  modelValue: string;
  readonly?: boolean;
  title?: string;
  providers?: ProviderMetadata[];
  settings?: CredentialSummary[];
  sourcePath?: string | null;
}>();

// map stored credential summaries to completion setting refs, defaulting unkinded entries to secret.
function settingRefs(): WdlSettingRef[] {
  return (props.settings ?? []).map((setting) => ({
    scope: setting.scope,
    name: setting.name,
    kind: setting.kind ?? "secret",
  }));
}

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const editorContainer = ref<HTMLElement | null>(null);
const diagnostics = ref<WdlDiagnostic[]>([]);
// editability is reconfigurable so a readonly toggle after mount takes effect live.
const editableCompartment = new Compartment();
let view: EditorView | null = null;
let disposeEditorTheme: (() => void) | null = null;
const title = props.title ?? "WDL";
const app = useAppStore();
let diagnosticsRequest = 0;
const WDL_LINT_DELAY_MS = 1500;

const diagnosticCounts = computed(() => ({
  errors: diagnostics.value.filter((diagnostic) => diagnostic.severity === "error").length,
  warnings: diagnostics.value.filter((diagnostic) => diagnostic.severity === "warning").length,
}));
const diagnosticSummary = computed(() => {
  const { errors, warnings } = diagnosticCounts.value;

  if (errors > 0) {
    return `${String(errors)} error${errors === 1 ? "" : "s"}`;
  }

  if (warnings > 0) {
    return `${String(warnings)} warning${warnings === 1 ? "" : "s"}`;
  }

  return "Clean";
});
const diagnosticSummaryClass = computed(() => {
  if (diagnosticCounts.value.errors > 0) {
    return "error";
  }

  if (diagnosticCounts.value.warnings > 0) {
    return "warning";
  }

  return "clean";
});

// async linter backed by the rust runinator-wdl compiler, so editor diagnostics match
// what the importer would report. codemirror debounces this by default.
const wdlLinter = linter(
  async (linterView): Promise<Diagnostic[]> => {
    const source = linterView.state.doc.toString();
    const docLength = linterView.state.doc.length;
    let nextDiagnostics;

    try {
      nextDiagnostics = await refreshDiagnostics(source);
    } catch {
      return [];
    }

    return nextDiagnostics.map((diagnostic) => {
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
  },
  { delay: WDL_LINT_DELAY_MS },
);

async function refreshDiagnostics(source: string): Promise<WdlDiagnostic[]> {
  const request = ++diagnosticsRequest;
  const nextDiagnostics = await analyzeWdl(source, props.sourcePath);

  if (request === diagnosticsRequest) {
    diagnostics.value = nextDiagnostics;
  }

  return nextDiagnostics;
}

function diagnosticKey(diagnostic: WdlDiagnostic) {
  return `${diagnostic.severity}:${String(diagnostic.start)}:${String(diagnostic.end)}:${diagnostic.message}`;
}

function goToDiagnostic(diagnostic: WdlDiagnostic) {
  if (!view) {
    return;
  }

  const position = Math.min(Math.max(diagnostic.start, 0), view.state.doc.length);
  view.dispatch({
    selection: { anchor: position },
    effects: EditorView.scrollIntoView(position, { y: "center" }),
  });
  view.focus();
}

async function formatDocument() {
  if (!view || props.readonly) {
    return;
  }

  const source = view.state.doc.toString();
  let formatted: string;

  try {
    formatted = await formatWdl(source);
  } catch (err) {
    app.setError(`WDL format error: ${String(err)}`);
    return;
  }

  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: formatted },
  });
  emit("update:modelValue", formatted);
  await refreshDiagnostics(formatted);
}

onMounted(() => {
  if (!editorContainer.value) {
    return;
  }

  const editorTheme = osCodeMirrorTheme();

  const startState = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      editorTheme.extension,
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
      wdl(wdlProviderCompletionSource(() => props.providers ?? [], settingRefs)),
      wdlHoverTooltip(() => props.providers ?? [], settingRefs),
      wdlLinter,
      editableCompartment.of(EditorView.editable.of(!props.readonly)),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          emit("update:modelValue", update.state.doc.toString());
        }

        if (!props.readonly && shouldStartCompletion(update)) {
          startCompletion(update.view);
        }
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
        ".cm-tooltip": {
          border: "1px solid var(--border-strong)",
          borderRadius: "6px",
          boxShadow: "var(--workflow-menu-shadow)",
        },
        ".wdl-hover": {
          maxWidth: "420px",
          padding: "8px 10px",
          fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
          fontSize: "12px",
          lineHeight: "1.35",
          color: "var(--text)",
        },
        ".wdl-hover-title": {
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
          fontWeight: "700",
          color: "var(--text)",
        },
        ".wdl-hover-meta": {
          marginTop: "3px",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
          color: "var(--text-muted)",
        },
        ".wdl-hover-docs": {
          marginTop: "7px",
          color: "var(--text-subtle)",
          whiteSpace: "pre-line",
        },
      }),
    ],
  });

  view = new EditorView({
    state: startState,
    parent: editorContainer.value,
  });
  disposeEditorTheme = editorTheme.install(view);
});

watch(
  () => props.modelValue,
  (newValue) => {
    if (view && newValue !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: newValue },
      });
    }
  },
);

watch(
  () => props.readonly,
  (readonly) => {
    view?.dispatch({ effects: editableCompartment.reconfigure(EditorView.editable.of(!readonly)) });
  },
);

onBeforeUnmount(() => {
  disposeEditorTheme?.();

  if (view) {
    view.destroy();
  }
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
</script>

<style scoped>
.wdl-editor-shell {
  display: flex;
  flex: 1 1 auto;
  height: auto;
  min-height: 0;
  min-width: 0;
  flex-direction: column;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background-color: var(--surface);
  overflow: hidden;
}

.wdl-editor-title {
  padding: 8px 10px;
  font-weight: 600;
  user-select: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.wdl-editor-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.wdl-diagnostic-summary {
  border-radius: 999px;
  padding: 2px 7px;
  font-size: 11px;
  font-weight: 700;
}

.wdl-diagnostic-summary.clean {
  background: var(--success-bg);
  color: var(--success-fg);
}

.wdl-diagnostic-summary.warning {
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.wdl-diagnostic-summary.error {
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.wdl-editor-title button {
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-subtle);
  color: var(--text);
  cursor: pointer;
  font: inherit;
  font-weight: 600;
  padding: 3px 8px;
}

.wdl-editor-title button:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.wdl-editor-container {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  border-top: 1px solid var(--border-subtle);
  overflow: hidden;
}

.wdl-diagnostics {
  flex: 0 0 136px;
  min-height: 104px;
  max-height: 180px;
  overflow: auto;
  border-top: 1px solid var(--border-subtle);
  background: var(--surface-subtle);
}

.wdl-diagnostics table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.wdl-diagnostics th,
.wdl-diagnostics td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
  vertical-align: top;
}

.wdl-diagnostics th {
  position: sticky;
  top: 0;
  background: var(--surface-sunken);
  color: var(--text-subtle);
  font-weight: 700;
}

.wdl-diagnostics tbody tr {
  cursor: pointer;
}

.wdl-diagnostics tbody tr:hover {
  background: var(--surface-hover);
}

.wdl-diagnostics tbody tr.error {
  box-shadow: inset 3px 0 var(--danger-solid);
}

.wdl-diagnostics tbody tr.warning {
  box-shadow: inset 3px 0 var(--warn-solid);
}

.wdl-diagnostics td:nth-child(2) {
  min-width: 220px;
}

.wdl-diagnostics td:nth-child(3),
.wdl-diagnostics td:nth-child(4) {
  width: 56px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.wdl-diagnostic-severity {
  display: inline-block;
  min-width: 52px;
  border-radius: 4px;
  padding: 1px 5px;
  text-align: center;
  text-transform: capitalize;
}

.wdl-diagnostic-severity.error {
  background: var(--danger-bg);
  color: var(--danger-fg);
}

.wdl-diagnostic-severity.warning {
  background: var(--warning-bg);
  color: var(--warning-fg);
}

.wdl-diagnostics-empty {
  padding: 7px 10px;
  color: var(--text-muted);
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
