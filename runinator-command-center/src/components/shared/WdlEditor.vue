<template>
  <section class="wdl-editor-shell" @mousedown.stop @click.stop>
    <header class="wdl-editor-title">
      <span>{{ title }}</span>
      <div class="wdl-editor-actions">
        <span :class="['wdl-diagnostic-summary', diagnosticSummaryClass]">{{ diagnosticSummary }}</span>
        <button type="button" :disabled="readonly" @click.stop.prevent="formatDocument">Format</button>
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
              <span :class="['wdl-diagnostic-severity', diagnostic.severity]">{{ diagnostic.severity }}</span>
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
import { computed, ref, onMounted, watch, onBeforeUnmount } from 'vue';
import { EditorView, basicSetup } from 'codemirror';
import { Compartment, EditorState } from '@codemirror/state';
import { linter, type Diagnostic } from '@codemirror/lint';
import { wdl } from '../../utils/codemirror-lang-wdl';
import { wdlProviderCompletionSource } from '../../utils/wdl-completion';
import { analyzeWdl, formatWdl } from '../../api/commandCenterApi';
import { useAppStore } from '../../stores/app';
import type { CredentialSummary, ProviderMetadata, WdlDiagnostic, WdlSettingRef } from '../../types/models';

const props = defineProps<{
  modelValue: string;
  readonly?: boolean;
  title?: string;
  providers?: ProviderMetadata[];
  settings?: CredentialSummary[];
}>();

// map stored credential summaries to completion setting refs, defaulting unkinded entries to secret.
function settingRefs(): WdlSettingRef[] {
  return (props.settings ?? []).map((setting) => ({
    scope: setting.scope,
    name: setting.name,
    kind: setting.kind ?? "secret"
  }));
}

const emit = defineEmits<{
  "update:modelValue": [value: string]
}>();

const editorContainer = ref<HTMLElement | null>(null);
const diagnostics = ref<WdlDiagnostic[]>([]);
// editability is reconfigurable so a readonly toggle after mount takes effect live.
const editableCompartment = new Compartment();
let view: EditorView | null = null;
const title = props.title ?? "WDL";
const app = useAppStore();
let diagnosticsRequest = 0;

const diagnosticCounts = computed(() => ({
  errors: diagnostics.value.filter((diagnostic) => diagnostic.severity === "error").length,
  warnings: diagnostics.value.filter((diagnostic) => diagnostic.severity === "warning").length
}));
const diagnosticSummary = computed(() => {
  const { errors, warnings } = diagnosticCounts.value;
  if (errors > 0) return `${errors} error${errors === 1 ? "" : "s"}`;
  if (warnings > 0) return `${warnings} warning${warnings === 1 ? "" : "s"}`;
  return "Clean";
});
const diagnosticSummaryClass = computed(() => {
  if (diagnosticCounts.value.errors > 0) return "error";
  if (diagnosticCounts.value.warnings > 0) return "warning";
  return "clean";
});

// async linter backed by the rust runinator-wdl compiler, so editor diagnostics match
// what the importer would report. codemirror debounces this by default.
const wdlLinter = linter(async (linterView): Promise<Diagnostic[]> => {
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
});

async function refreshDiagnostics(source: string): Promise<WdlDiagnostic[]> {
  const request = ++diagnosticsRequest;
  const nextDiagnostics = await analyzeWdl(source);
  if (request === diagnosticsRequest) {
    diagnostics.value = nextDiagnostics;
  }
  return nextDiagnostics;
}

function diagnosticKey(diagnostic: WdlDiagnostic) {
  return `${diagnostic.severity}:${diagnostic.start}:${diagnostic.end}:${diagnostic.message}`;
}

function goToDiagnostic(diagnostic: WdlDiagnostic) {
  if (!view) return;
  const position = Math.min(Math.max(diagnostic.start, 0), view.state.doc.length);
  view.dispatch({
    selection: { anchor: position },
    effects: EditorView.scrollIntoView(position, { y: "center" })
  });
  view.focus();
}

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
  await refreshDiagnostics(formatted);
}

onMounted(() => {
  if (!editorContainer.value) return;

  const startState = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      wdl(wdlProviderCompletionSource(() => props.providers ?? [], settingRefs)),
      wdlLinter,
      editableCompartment.of(EditorView.editable.of(!props.readonly)),
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

watch(() => props.readonly, (readonly) => {
  view?.dispatch({ effects: editableCompartment.reconfigure(EditorView.editable.of(!readonly)) });
});

onBeforeUnmount(() => {
  if (view) {
    view.destroy();
  }
});
</script>

<style scoped>
.wdl-editor-shell {
  display: flex;
  flex: 1 1 auto;
  height: auto;
  min-height: 0;
  min-width: 0;
  flex-direction: column;
  border: 1px solid #ccd4dd;
  border-radius: 6px;
  background-color: #fff;
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
  background: #e8f7ef;
  color: #11653b;
}

.wdl-diagnostic-summary.warning {
  background: #fff4cc;
  color: #8a5a00;
}

.wdl-diagnostic-summary.error {
  background: #fde8e8;
  color: #b91c1c;
}

.wdl-editor-title button {
  border: 1px solid #b8c3cf;
  border-radius: 4px;
  background: #f7f9fb;
  color: #1c2938;
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
  border-top: 1px solid #e3e8ee;
  overflow: hidden;
}

.wdl-diagnostics {
  flex: 0 0 136px;
  min-height: 104px;
  max-height: 180px;
  overflow: auto;
  border-top: 1px solid #e3e8ee;
  background: #fbfcfe;
}

.wdl-diagnostics table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.wdl-diagnostics th,
.wdl-diagnostics td {
  padding: 5px 8px;
  border-bottom: 1px solid #e6ebf1;
  text-align: left;
  vertical-align: top;
}

.wdl-diagnostics th {
  position: sticky;
  top: 0;
  background: #f2f5f8;
  color: #4b5663;
  font-weight: 700;
}

.wdl-diagnostics tbody tr {
  cursor: pointer;
}

.wdl-diagnostics tbody tr:hover {
  background: #eef4ff;
}

.wdl-diagnostics tbody tr.error {
  box-shadow: inset 3px 0 #dc2626;
}

.wdl-diagnostics tbody tr.warning {
  box-shadow: inset 3px 0 #d97706;
}

.wdl-diagnostics td:nth-child(2) {
  min-width: 220px;
}

.wdl-diagnostics td:nth-child(3),
.wdl-diagnostics td:nth-child(4) {
  width: 56px;
  color: #4b5663;
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
  background: #fde8e8;
  color: #b91c1c;
}

.wdl-diagnostic-severity.warning {
  background: #fff4cc;
  color: #8a5a00;
}

.wdl-diagnostics-empty {
  padding: 7px 10px;
  color: #66717e;
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
