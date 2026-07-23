<template>
  <section class="editor-shell" @mousedown.stop @click.stop>
    <header class="editor-shell-title !px-2.5 !py-2 !font-semibold">
      <span>{{ title }}</span>
      <div class="inline-flex min-w-0 items-center gap-2">
        <span
          class="rounded-pill px-1.5 py-0.5 text-[11px] font-bold"
          :class="diagnosticSummaryClass"
          >{{ diagnosticSummary }}</span
        >
        <button
          type="button"
          class="editor-shell-title-btn !bg-surface-subtle disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="readonly"
          @click.stop.prevent="formatDocument"
        >
          Format
        </button>
      </div>
    </header>
    <div ref="editorContainer" class="editor-shell-container wdl-editor-container"></div>
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
              <span
                class="inline-block min-w-[52px] rounded px-1 py-px text-center capitalize"
                :class="severityBadgeClass(diagnostic.severity)"
                >{{ diagnostic.severity }}</span
              >
            </td>
            <td>{{ diagnostic.message }}</td>
            <td>{{ diagnostic.line }}</td>
            <td>{{ diagnostic.column }}</td>
          </tr>
        </tbody>
      </table>
      <div v-else class="px-2.5 py-1.5 text-xs text-fg-muted">No WDL diagnostics.</div>
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
    return "bg-danger-bg text-danger-fg";
  }

  if (diagnosticCounts.value.warnings > 0) {
    return "bg-warning-bg text-warning-fg";
  }

  return "bg-success-bg text-success-fg";
});

function severityBadgeClass(severity: string): string {
  if (severity === "error") {
    return "bg-danger-bg text-danger-fg";
  }

  if (severity === "warning") {
    return "bg-warning-bg text-warning-fg";
  }

  return "bg-surface-muted text-fg-muted";
}

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
