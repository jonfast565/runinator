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
            <button class="watch-remove" title="Remove" @click="workflows.removeWatchExpression(expr)">×</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="watch-empty">No expressions yet. Add one to track context values across steps.</div>
  </details>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkflowsStore } from "../../stores/workflows";
import { evaluatePointer, type PointerResult } from "../../utils/json-pointer";

const workflows = useWorkflowsStore();
const draft = ref("");

const expressions = computed(() => workflows.watchExpressionsForActiveWorkflow);
const context = computed(() => workflows.debugState?.context_json ?? null);

function evaluate(expr: string): PointerResult {
  return evaluatePointer(context.value, expr);
}

function formatValue(result: PointerResult): string {
  if (!result.exists) return "—";
  if (result.value === null) return "null";
  if (typeof result.value === "object") return JSON.stringify(result.value);
  return String(result.value);
}

function onAdd() {
  const expr = draft.value.trim();
  if (!expr) return;
  workflows.addWatchExpression(expr);
  draft.value = "";
}
</script>

<style scoped>
.watch-panel {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 5px 8px;
  font-size: 12px;
  margin-bottom: 8px;
}
.watch-panel summary {
  cursor: pointer;
  font-weight: 600;
  color: var(--text-subtle);
  user-select: none;
}
.watch-add {
  display: flex;
  gap: 6px;
  margin: 6px 0;
}
.watch-add input {
  flex: 1;
  padding: 4px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  font-size: 11px;
}
.watch-add button {
  padding: 4px 10px;
  font-size: 11px;
}
.watch-table {
  width: 100%;
  border-collapse: collapse;
  font-family: "SFMono-Regular", Consolas, monospace;
}
.watch-table th,
.watch-table td {
  padding: 3px 6px;
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
}
.watch-table th {
  font-size: 10px;
  color: var(--text-muted);
  font-weight: 600;
}
.watch-path {
  color: var(--text);
  font-weight: 500;
  white-space: nowrap;
}
.watch-value {
  color: var(--text);
  word-break: break-all;
}
.watch-value.empty {
  color: var(--text-faint);
  font-style: italic;
}
.watch-actions {
  width: 24px;
  text-align: right;
}
.watch-remove {
  background: transparent;
  border: 0;
  color: var(--text-faint);
  font-size: 14px;
  cursor: pointer;
  padding: 0 4px;
}
.watch-remove:hover {
  color: var(--danger-fg);
}
.watch-empty {
  padding: 6px 0;
  color: var(--text-faint);
  font-style: italic;
  font-size: 11px;
}
</style>
