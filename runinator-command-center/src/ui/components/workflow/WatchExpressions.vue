<template>
  <details class="watch-panel">
    <summary>Watch expressions ({{ expressions.length }})</summary>
    <div class="watch-add">
      <input
        v-model="draft"
        placeholder="JSON pointer (e.g. /workflow/state/map/item or /params/name, legacy /input/name)"
        @keyup.enter="onAdd"
      />
      <button :disabled="!draft.trim()" @click="onAdd">+ Add watch</button>
    </div>
    <table v-if="expressions.length" class="watch-table">
      <thead>
        <tr>
          <th>Path</th>
          <th>Value</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="expr in expressions" :key="expr">
          <td class="watch-path">{{ expr }}</td>
          <td class="watch-value" :class="{ empty: !evaluate(expr).exists }">
            {{ formatValue(evaluate(expr)) }}
          </td>
          <td class="watch-actions">
            <button
              class="watch-remove"
              title="Remove"
              @click="workflows.removeWatchExpression(expr)"
            >
              ×
            </button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="watch-empty">
      No expressions yet. Add one to track context values across steps.
    </div>
  </details>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../../ui/adapters/pinia/workflows";
import { evaluatePointer, type PointerResult } from "../../../core/utils/json-pointer";
import { displayValue } from "../../../core/utils/values";

const workflows = useWorkflowsStore();
const draft = ref("");

const expressions = computed(() => workflows.watchExpressionsForActiveWorkflow);
const context = computed(() => workflows.debugState?.context_json ?? null);

function evaluate(expr: string): PointerResult {
  return evaluatePointer(context.value, expr);
}

function formatValue(result: PointerResult): string {
  if (!result.exists) {
    return "—";
  }

  if (result.value === null) {
    return "null";
  }

  if (typeof result.value === "object") {
    return JSON.stringify(result.value);
  }

  return displayValue(result.value);
}

function onAdd() {
  const expr = draft.value.trim();

  if (!expr) {
    return;
  }

  workflows.addWatchExpression(expr);
  draft.value = "";
}
</script>

