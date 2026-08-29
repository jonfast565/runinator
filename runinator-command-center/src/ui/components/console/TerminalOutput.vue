<template>
  <pre
    v-if="output.kind === 'text'"
    class="m-0 whitespace-pre-wrap break-words"
    :class="toneClass"
    >{{ output.text }}</pre>
  <pre
    v-else-if="output.kind === 'json'"
    class="m-0 whitespace-pre-wrap break-words text-fg-inverse-muted"
    >{{ pretty(output.value) }}</pre>
  <div v-else class="overflow-x-auto">
    <DataTable bare table-class="border-collapse">
      <thead>
        <tr>
          <th
            v-for="column in output.columns"
            :key="column"
            class="border-b border-fg-inverse-faint px-2 py-0.5 text-left font-semibold text-fg-inverse-faint"
          >
            {{ column }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, index) in output.rows" :key="index">
          <td v-for="(value, column) in row" :key="column" class="px-2 py-0.5 align-top">
            {{ value }}
          </td>
        </tr>
      </tbody>
    </DataTable>
    <p v-if="output.rows.length === 0" class="m-0 px-2 text-fg-inverse-faint">(none)</p>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { pretty } from "../../../core/utils/format";
import type { ConsoleOutput } from "../../../core/console/types";

const props = defineProps<{ output: ConsoleOutput }>();

// a tone is the terminal's whole vocabulary for "this went wrong" — there is no badge here, the
// same way there is none in a shell.
const toneClass = computed(() => {
  if (props.output.kind !== "text") {
    return "";
  }

  return {
    error: "text-danger-fg",
    success: "text-accent-pulse",
    muted: "text-fg-inverse-faint",
  }[props.output.tone ?? "muted"];
});
</script>
