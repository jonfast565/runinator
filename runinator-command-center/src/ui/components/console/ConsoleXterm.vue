<template>
  <XtermSurface
    ref="surface"
    :content="content"
    :readonly="false"
    aria-label="Runinator console prompt"
    @data="onData"
  />
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { cellReference } from "../../../core/domain/models";
import type { ConsoleCell } from "../../../core/domain/models";
import type { ConsoleOutput } from "../../../core/console/types";
import { ctlComplete, ctlIsSubmittable } from "../../../core/console/wasm-engine";
import type { TranscriptEntry } from "../../../core/services/console-terminal";
import { pretty } from "../../../core/utils/format";
import XtermSurface from "./XtermSurface.vue";

const props = defineProps<{
  entries: TranscriptEntry[];
  cells: ConsoleCell[];
  history: string[];
  busy: boolean;
}>();
const emit = defineEmits<{ submit: [line: string]; stop: []; clear: [] }>();
const surface = ref<InstanceType<typeof XtermSurface> | null>(null);
const buffer = ref("");
const historyIndex = ref(-1);
const draft = ref("");
const completionNote = ref("");

const RESET = "\x1b[0m";
const MUTED = "\x1b[38;2;145;155;170m";
const ACCENT = "\x1b[38;2;45;224;209m";
const ERROR = "\x1b[38;2;255;155;145m";
const SUCCESS = "\x1b[38;2;116;217;159m";

const content = computed(() => {
  const lines = [
    `${MUTED}runinator console — bare input is REXRAP; :help lists runinatorctl commands.${RESET}`,
  ];

  for (const entry of props.entries) {
    const sigil = entry.kind === "command" ? ":" : "›";
    const input = entry.kind === "command" ? entry.input.slice(1) : entry.input;
    lines.push("", `${ACCENT}${sigil}${RESET} ${safe(input)}`);

    for (const output of entry.outputs) {
      lines.push(...formatOutput(output));
    }

    const cell = entry.cellId ? props.cells.find((candidate) => candidate.id === entry.cellId) : null;

    if (cell) {
      const how =
        cell.kind === "workflow"
          ? `ran as workflow run ${cell.workflow_run_id ?? ""}`
          : `evaluated as ${cell.kind ?? ""}`;
      lines.push(`${MUTED}${safe(`${how.trim()} → ${cellReference(cell)}`)}${RESET}`);

      if (cell.error) {
        lines.push(`${ERROR}${safe(cell.error)}${RESET}`);
      } else if (cell.result !== null && cell.result !== undefined) {
        lines.push(`${MUTED}${safe(pretty(cell.result))}${RESET}`);
      }
    }

    if (entry.error) {
      lines.push(`${ERROR}${safe(entry.error)}${RESET}`);
    } else if (entry.status === "running") {
      lines.push(`${MUTED}running…${RESET}`);
    }
  }

  if (completionNote.value) {
    lines.push(`${MUTED}${safe(completionNote.value)}${RESET}`);
  }

  const sigil = buffer.value.trimStart().startsWith(":") ? ":" : "›";
  const prompt = props.busy ? `${MUTED}running… Ctrl+C stops${RESET}` : safe(buffer.value);
  lines.push("", `${ACCENT}${sigil}${RESET} ${prompt}`);
  return lines.join("\r\n");
});

function safe(value: string): string {
  return value.replaceAll("\x1b", "").replaceAll("\r\n", "\n").replaceAll("\n", "\r\n");
}

function formatOutput(output: ConsoleOutput): string[] {
  if (output.kind === "text") {
    const color = output.tone === "error" ? ERROR : output.tone === "success" ? SUCCESS : MUTED;
    return [`${color}${safe(output.text)}${RESET}`];
  }

  if (output.kind === "json") {
    return [`${MUTED}${safe(pretty(output.value))}${RESET}`];
  }

  const widths = output.columns.map((column, index) =>
    Math.max(column.length, ...output.rows.map((row) => (row[index] ?? "").length)),
  );
  const row = (values: string[]) =>
    values.map((value, index) => value.padEnd(widths[index] ?? value.length)).join("  ").trimEnd();
  return [
    `${MUTED}${safe(row(output.columns))}${RESET}`,
    `${MUTED}${widths.map((width) => "─".repeat(width)).join("  ")}${RESET}`,
    ...(output.rows.length ? output.rows.map((values) => safe(row(values))) : [`${MUTED}(none)${RESET}`]),
  ];
}

function onData(data: string) {
  if (data === "\x03") {
    buffer.value = "";
    emit("stop");
    return;
  }

  if (data === "\x0c") {
    emit("clear");
    return;
  }

  if (props.busy) {
    return;
  }

  if (data === "\x1b[A" || data === "\x1b[B") {
    recall(data === "\x1b[A");
    return;
  }

  if (data === "\x7f") {
    const characters = Array.from(buffer.value);
    characters.pop();
    buffer.value = characters.join("");
    return;
  }

  if (data === "\t") {
    applyCompletion();
    return;
  }

  if (data === "\r") {
    if (ctlIsSubmittable(buffer.value)) {
      emit("submit", buffer.value);
      buffer.value = "";
      historyIndex.value = -1;
      completionNote.value = "";
    } else if (buffer.value.trim()) {
      buffer.value += "\n";
    }

    return;
  }

  if (!data.startsWith("\x1b")) {
    buffer.value += data.replaceAll("\r", "\n");
    completionNote.value = "";
  }
}

function recall(up: boolean) {
  if (!props.history.length) {
    return;
  }

  if (historyIndex.value === -1) {
    if (!up) {
      return;
    }

    draft.value = buffer.value;
    historyIndex.value = props.history.length;
  }

  const next = historyIndex.value + (up ? -1 : 1);

  if (next >= props.history.length) {
    historyIndex.value = -1;
    buffer.value = draft.value;
    return;
  }

  historyIndex.value = Math.max(0, next);
  buffer.value = props.history[historyIndex.value] ?? "";
}

function applyCompletion() {
  const result = ctlComplete(buffer.value);

  if (result.options.length === 1) {
    buffer.value = `${buffer.value.slice(0, result.start)}${result.options[0]} `;
    completionNote.value = "";
  } else {
    const offered = result.options.join("    ");
    completionNote.value = offered ? offered : (result.hint ?? "");
  }
}

async function focusSurface(): Promise<void> {
  await surface.value?.focus();
}

watch(
  () => props.busy,
  (busy) => {
    if (!busy) {
      void focusSurface();
    }
  },
);

defineExpose({ focus: focusSurface });
</script>
