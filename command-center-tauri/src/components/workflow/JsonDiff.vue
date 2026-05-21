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

const props = defineProps<{
  before: any;
  after: any;
  title?: string;
  open?: boolean;
}>();

type DiffKind = "added" | "removed" | "changed";
type DiffRow = { path: string; kind: DiffKind; before: any; after: any };

const title = computed(() => props.title ?? "Diff (input → output)");

const rows = computed<DiffRow[]>(() => {
  const out: DiffRow[] = [];
  walk("", props.before, props.after, out);
  return out;
});

function isObject(v: any): v is Record<string, any> {
  return v != null && typeof v === "object" && !Array.isArray(v);
}

function walk(path: string, a: any, b: any, out: DiffRow[]) {
  if (deepEqual(a, b)) return;
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
      walk(`${path}[${i}]`, a[i], b[i], out);
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

function deepEqual(a: any, b: any): boolean {
  if (a === b) return true;
  if (a == null || b == null) return a === b;
  if (typeof a !== typeof b) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((v, i) => deepEqual(v, b[i]));
  }
  if (typeof a === "object") {
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => deepEqual(a[k], b[k]));
  }
  return false;
}

function formatValue(v: any): string {
  if (v === undefined) return "—";
  if (v === null) return "null";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}
</script>

<style scoped>
.json-diff {
  border: 1px solid #d8e2ec;
  border-radius: 6px;
  background: #fbfcfe;
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
  color: #475569;
  user-select: none;
}
.json-diff-empty {
  padding: 8px 0;
  color: #94a3b8;
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
  border-bottom: 1px solid #e2e8f0;
  text-align: left;
  vertical-align: top;
  word-break: break-all;
}
.json-diff-table th {
  font-size: 10px;
  color: #64748b;
  font-weight: 600;
}
.path {
  font-weight: 500;
  color: #1e293b;
  white-space: nowrap;
}
.added .cell-after {
  background: #dcfce7;
  color: #166534;
}
.removed .cell-before {
  background: #fee2e2;
  color: #991b1b;
}
.changed .cell-before {
  background: #fef3c7;
  color: #92400e;
}
.changed .cell-after {
  background: #fef3c7;
  color: #92400e;
}
</style>
