<template>
  <div ref="scroller" class="flex-1 overflow-auto px-3 py-2 font-mono text-[12px] leading-6">
    <p class="m-0 text-fg-inverse-faint">
      runinator console — a bare line is WDL, a `:` line is a runinatorctl command. `:help` lists
      both, Tab completes.
    </p>

    <article v-for="entry in entries" :key="entry.id" class="mt-3">
      <div class="flex items-start gap-2">
        <span class="select-none text-accent-pulse" aria-hidden="true">{{
          entry.kind === "command" ? ":" : "›"
        }}</span>
        <pre class="m-0 flex-1 whitespace-pre-wrap break-words text-fg-inverse">{{
          entry.kind === "command" ? entry.input.slice(1) : entry.input
        }}</pre>
        <span v-if="entry.status === 'running'" class="text-fg-inverse-faint">running…</span>
      </div>

      <div class="mt-1 pl-4">
        <TerminalOutput
          v-for="(output, index) in entry.outputs"
          :key="index"
          :output="output"
          class="mb-1"
        />

        <!-- a cell's result is read from the session rather than copied into the transcript, so a
             run that finishes a minute later still lands in the line that started it. -->
        <template v-if="entry.cellId">
          <p v-if="cellFor(entry)?.kind" class="m-0 text-fg-inverse-faint">
            {{ cellNote(entry) }}
          </p>
          <pre v-if="cellFor(entry)?.error" class="m-0 whitespace-pre-wrap text-danger-fg">{{
            cellFor(entry)?.error
          }}</pre>
          <pre
            v-else-if="hasResult(entry)"
            class="m-0 whitespace-pre-wrap break-words text-fg-inverse-muted"
            >{{ pretty(cellFor(entry)?.result ?? null) }}</pre>
        </template>

        <pre v-if="entry.error" class="m-0 whitespace-pre-wrap text-danger-fg">{{
          entry.error
        }}</pre>
      </div>
    </article>
  </div>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import TerminalOutput from "./TerminalOutput.vue";
import { pretty } from "../../../core/utils/format";
import { cellReference } from "../../../core/domain/models";
import type { ConsoleCell } from "../../../core/domain/models";
import type { TranscriptEntry } from "../../../core/services/console-terminal";

const props = defineProps<{ entries: TranscriptEntry[]; cells: ConsoleCell[] }>();

const scroller = ref<HTMLElement | null>(null);

function cellFor(entry: TranscriptEntry): ConsoleCell | undefined {
  return props.cells.find((cell) => cell.id === entry.cellId);
}

function hasResult(entry: TranscriptEntry): boolean {
  const result = cellFor(entry)?.result;
  return result !== null && result !== undefined;
}

// says how the cell was answered and what name its result took, which is the console's one piece of
// state a later line depends on.
function cellNote(entry: TranscriptEntry): string {
  const cell = cellFor(entry);

  if (!cell) {
    return "";
  }

  const how =
    cell.kind === "workflow"
      ? `ran as workflow run ${cell.workflow_run_id ?? ""}`
      : `evaluated as ${cell.kind ?? ""}`;
  return `${how.trim()} → ${cellReference(cell)}`;
}

// a terminal always shows its newest line, so the scroller follows the transcript.
watch(
  () =>
    props.entries
      .map((entry) => `${entry.id}:${String(entry.outputs.length)}:${entry.status}`)
      .join(),
  async () => {
    await nextTick();

    if (scroller.value) {
      scroller.value.scrollTop = scroller.value.scrollHeight;
    }
  },
);
</script>
