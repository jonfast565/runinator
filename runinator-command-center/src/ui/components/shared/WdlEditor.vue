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
import { getTextEditorHostFactory } from "../../../core/platform";
import type { TextEditorDiagnostic } from "../../../core/platform/text-editor";
import { useAppStore } from "../../../ui/adapters/pinia/app";
import type { CredentialSummary, ProviderMetadata, WdlSettingRef } from "../../../core/domain/models";
import type { CodeMirrorHostOptions } from "../../adapters/codemirror/text-editor-host";

const props = defineProps<{
  modelValue: string;
  readonly?: boolean;
  title?: string;
  providers?: ProviderMetadata[];
  settings?: CredentialSummary[];
  sourcePath?: string | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const editorContainer = ref<HTMLElement | null>(null);
const diagnostics = ref<TextEditorDiagnostic[]>([]);
const title = props.title ?? "WDL";
const app = useAppStore();
let host: ReturnType<ReturnType<typeof getTextEditorHostFactory>["create"]> | null = null;

function settingRefs(): WdlSettingRef[] {
  return (props.settings ?? []).map((setting) => ({
    scope: setting.scope,
    name: setting.name,
    kind: setting.kind ?? "secret",
  }));
}

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

function diagnosticKey(diagnostic: TextEditorDiagnostic) {
  return `${diagnostic.severity}:${String(diagnostic.line)}:${String(diagnostic.column)}:${diagnostic.message}`;
}

function goToDiagnostic(diagnostic: TextEditorDiagnostic) {
  host?.goToPosition(diagnostic.line, diagnostic.column);
}

async function formatDocument() {
  if (!host || props.readonly) {
    return;
  }

  try {
    await host.formatDocument?.();
  } catch (err) {
    app.setError(`WDL format error: ${String(err)}`);
  }
}

onMounted(() => {
  if (!editorContainer.value) {
    return;
  }

  const options: CodeMirrorHostOptions = {
    language: "wdl",
    value: props.modelValue,
    readonly: props.readonly,
    sourcePath: props.sourcePath,
    onChange(value) {
      emit("update:modelValue", value);
    },
    onDiagnosticsChange(nextDiagnostics) {
      diagnostics.value = nextDiagnostics;
    },
    wdlContext: {
      providers: () => props.providers ?? [],
      settings: settingRefs,
      sourcePath: props.sourcePath,
    },
  };

  host = getTextEditorHostFactory().create(options);
  host.mount(editorContainer.value);
});

watch(
  () => props.modelValue,
  (newValue) => {
    host?.setValue(newValue, true);
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
