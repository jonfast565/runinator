<template>
  <details class="json-diff" :open="open">
    <summary>{{ title }}</summary>
    <div v-if="rows.length === 0" class="json-diff-empty">No differences</div>
    <table v-else class="json-diff-table">
      <thead>
        <tr>
          <th>Path</th>
          <th class="cell-before">Before</th>
          <th class="cell-after">After</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.path" :class="row.kind">
          <td class="path">{{ row.path || "(root)" }}</td>
          <td class="cell-before">{{ formatValue(row.before) }}</td>
          <td class="cell-after">{{ formatValue(row.after) }}</td>
        </tr>
      </tbody>
    </table>
  </details>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { displayValue } from "../../utils/values";

const props = defineProps<{
  before: unknown;
  after: unknown;
  title?: string;
  open?: boolean;
}>();

type DiffKind = "added" | "removed" | "changed";
interface DiffRow {
  path: string;
  kind: DiffKind;
  before: unknown;
  after: unknown;
}

const title = computed(() => props.title ?? "Diff (input → output)");

const rows = computed<DiffRow[]>(() => {
  const out: DiffRow[] = [];
  walk("", props.before, props.after, out);
  return out;
});

function isObject(v: unknown): v is Record<string, unknown> {
  return v != null && typeof v === "object" && !Array.isArray(v);
}

function walk(path: string, a: unknown, b: unknown, out: DiffRow[]) {
  if (deepEqual(a, b)) {
    return;
  }

  if (isObject(a) && isObject(b)) {
    const keys = new Set<string>([...Object.keys(a), ...Object.keys(b)]);

    for (const k of keys) {
      const next = path ? `${path}.${k}` : k;
      walk(next, a[k], b[k], out);
    }

    return;
  }

  if (Array.isArray(a) && Array.isArray(b)) {
    const len = Math.max(a.length, b.length);

    for (let i = 0; i < len; i++) {
      walk(`${path}[${String(i)}]`, a[i], b[i], out);
    }

    return;
  }

  if (a === undefined && b !== undefined) {
    out.push({ path, kind: "added", before: a, after: b });
    return;
  }

  if (a !== undefined && b === undefined) {
    out.push({ path, kind: "removed", before: a, after: b });
    return;
  }

  out.push({ path, kind: "changed", before: a, after: b });
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true;
  }

  if (a == null || b == null) {
    return a === b;
  }

  if (typeof a !== typeof b) {
    return false;
  }

  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }

    return a.every((v, i) => deepEqual(v, b[i]));
  }

  if (typeof a === "object" && typeof b === "object") {
    const recordA = a as Record<string, unknown>;
    const recordB = b as Record<string, unknown>;
    const ka = Object.keys(recordA);
    const kb = Object.keys(recordB);

    if (ka.length !== kb.length) {
      return false;
    }

    return ka.every((k) => deepEqual(recordA[k], recordB[k]));
  }

  return false;
}

function formatValue(v: unknown): string {
  if (v === undefined) {
    return "—";
  }

  if (v === null) {
    return "null";
  }

  if (typeof v === "string") {
    return JSON.stringify(v);
  }

  if (typeof v === "object") {
    return JSON.stringify(v);
  }

  return displayValue(v);
}
</script>

<style scoped>
.json-diff {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-subtle);
  padding: 6px 10px;
  font-size: 11px;
  margin-bottom: 8px;
  overflow: hidden;
}
.json-diff[open] {
  max-height: 220px;
  overflow: auto;
}
.json-diff summary {
  cursor: pointer;
  font-weight: 600;
  color: var(--text-subtle);
  user-select: none;
}
.json-diff-empty {
  padding: 8px 0;
  color: var(--text-faint);
  font-style: italic;
}
.json-diff-table {
  width: 100%;
  border-collapse: collapse;
  margin-top: 6px;
  font-family: "SFMono-Regular", Consolas, monospace;
}
.json-diff-table th,
.json-diff-table td {
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-subtle);
  text-align: left;
  vertical-align: top;
  word-break: break-all;
}
.json-diff-table th {
  font-size: 10px;
  color: var(--text-muted);
  font-weight: 600;
}
.path {
  font-weight: 500;
  color: var(--text);
  white-space: nowrap;
}
.added .cell-after {
  background: var(--success-bg);
  color: var(--success-fg);
}
.removed .cell-before {
  background: var(--danger-bg);
  color: var(--danger-fg);
}
.changed .cell-before {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
.changed .cell-after {
  background: var(--warning-bg);
  color: var(--warning-fg);
}
</style>
