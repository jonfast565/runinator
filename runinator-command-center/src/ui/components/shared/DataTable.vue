<script lang="ts">
export interface DataTableColumn<Row = Record<string, unknown>> {
  key: string;
  label: string;
  // enable header-click sorting for this column.
  sortable?: boolean;
  align?: "left" | "right" | "center";
  // fixed column width, e.g. "120px" or "20%".
  width?: string;
  // custom value accessor for sorting/display; defaults to row[key].
  value?: (row: Row) => unknown;
  // 'low' columns are hidden on mobile in scroll mode to reduce horizontal overflow.
  priority?: "high" | "low";
}
</script>

<script setup lang="ts" generic="Row">
import { computed, ref, watch } from "vue";
import Icon, { type IconName } from "./Icon.vue";
import EmptyState from "./EmptyState.vue";
import { useBreakpoint } from "../../composables/useBreakpoint";
import { displayValue } from "../../../core/utils/values";

// dual-mode table. with `columns` it renders a sortable/paginated/selectable table; without
// columns it stays a plain scroll wrapper so existing hand-written <table> slots keep working.
const props = withDefaults(
  defineProps<{
    columns?: DataTableColumn<Row>[];
    rows?: Row[];
    // row identity: a field name or an accessor. used for :key and selection.
    rowKey?: string | ((row: Row) => string | number);
    selectedKey?: string | number | null;
    // client-side page size; 0 disables pagination.
    pageSize?: number;
    compact?: boolean;
    // per-row css classes (e.g. danger/success/muted) mirroring the shared tables.css variants.
    rowClass?: (row: Row) => string | Record<string, boolean>;
    emptyTitle?: string;
    emptyDescription?: string;
    emptyIcon?: IconName;
    initialSortKey?: string;
    initialSortDir?: "asc" | "desc";
    // 'cards' renders label:value cards on mobile; 'scroll' keeps the table and hides low-priority columns.
    responsive?: "scroll" | "cards";
  }>(),
  {
    rows: () => [],
    pageSize: 0,
    compact: false,
    selectedKey: null,
    columns: undefined,
    rowKey: undefined,
    rowClass: undefined,
    emptyTitle: undefined,
    emptyDescription: undefined,
    emptyIcon: undefined,
    initialSortKey: undefined,
    initialSortDir: undefined,
    responsive: "scroll",
  },
);

const { isMobile } = useBreakpoint();
// switch to a stacked card layout on phones when the caller opts in via responsive="cards".
const cardMode = computed(() => props.responsive === "cards" && isMobile.value);

const emit = defineEmits<{ select: [row: Row] }>();

const sortKey = ref(props.initialSortKey ?? "");
const sortDir = ref<"asc" | "desc">(props.initialSortDir ?? "asc");
const page = ref(0);

function columnByKey(key: string): DataTableColumn<Row> | undefined {
  return props.columns?.find((column) => column.key === key);
}

function cellValue(row: Row, column: DataTableColumn<Row>): unknown {
  return column.value ? column.value(row) : (row as Record<string, unknown>)[column.key];
}

function keyForRow(row: Row, index: number): string | number {
  if (typeof props.rowKey === "function") {
    return props.rowKey(row);
  }

  const record = row as Record<string, unknown>;

  if (typeof props.rowKey === "string") {
    return displayValue(record[props.rowKey] ?? index);
  }

  return record.id as string | number;
}

const sortedRows = computed(() => {
  const column = columnByKey(sortKey.value);

  if (!column) {
    return props.rows;
  }

  const factor = sortDir.value === "asc" ? 1 : -1;
  return [...props.rows].sort(
    (left, right) => compareValues(cellValue(left, column), cellValue(right, column)) * factor,
  );
});

const pageCount = computed(() => {
  if (!props.pageSize) {
    return 1;
  }

  return Math.max(1, Math.ceil(sortedRows.value.length / props.pageSize));
});

const pagedRows = computed(() => {
  if (!props.pageSize) {
    return sortedRows.value;
  }

  const start = page.value * props.pageSize;
  return sortedRows.value.slice(start, start + props.pageSize);
});

// keep the current page in range when the row set shrinks (filtering, refresh).
watch([() => sortedRows.value.length, pageCount], () => {
  if (page.value > pageCount.value - 1) {
    page.value = pageCount.value - 1;
  }
});

function toggleSort(key: string) {
  if (sortKey.value === key) {
    sortDir.value = sortDir.value === "asc" ? "desc" : "asc";
    return;
  }

  sortKey.value = key;
  sortDir.value = "asc";
}

function rowClasses(row: Row, index: number): Record<string, boolean> {
  const base: Record<string, boolean> = {
    selected: props.selectedKey != null && keyForRow(row, index) === props.selectedKey,
  };
  const extra = props.rowClass?.(row);

  if (typeof extra === "string") {
    base[extra] = true;
  } else if (extra) {
    Object.assign(base, extra);
  }

  return base;
}

function displayCell(value: unknown): string {
  return displayValue(value);
}

const pageLabel = computed(() => `Page ${String(page.value + 1)} of ${String(pageCount.value)}`);

// natural-ish comparison: numbers numerically, everything else as case-insensitive strings.
function compareValues(left: unknown, right: unknown): number {
  if (left == null && right == null) {
    return 0;
  }

  if (left == null) {
    return -1;
  }

  if (right == null) {
    return 1;
  }

  if (typeof left === "number" && typeof right === "number") {
    return left - right;
  }

  return displayValue(left).toLowerCase().localeCompare(displayValue(right).toLowerCase());
}
</script>

<template>
  <div v-if="!columns" class="table-scroll">
    <slot />
  </div>
  <div v-else class="data-table">
    <!-- mobile card layout: each row becomes a stack of label:value pairs. -->
    <div v-if="cardMode" class="data-table-cards">
      <EmptyState
        v-if="!pagedRows.length && emptyTitle"
        compact
        :icon="emptyIcon"
        :title="emptyTitle"
        :description="emptyDescription"
      />
      <span v-else-if="!pagedRows.length" class="data-table-empty-text">No records.</span>
      <div
        v-for="(row, index) in pagedRows"
        :key="keyForRow(row, index)"
        class="data-card"
        :class="rowClasses(row, index)"
        @click="emit('select', row)"
      >
        <div v-for="column in columns" :key="column.key" class="data-card-row">
          <span class="data-card-label">{{ column.label }}</span>
          <span class="data-card-value" :class="column.align ? `align-${column.align}` : ''">
            <slot :name="`cell-${column.key}`" :row="row" :value="cellValue(row, column)">
              {{ displayCell(cellValue(row, column)) }}
            </slot>
          </span>
        </div>
      </div>
    </div>
    <div v-else class="table-scroll">
      <table :class="{ compact }">
        <thead>
          <tr>
            <th
              v-for="column in columns"
              :key="column.key"
              :class="[
                column.align ? `align-${column.align}` : '',
                column.priority === 'low' ? 'col-low' : '',
                { sortable: column.sortable },
              ]"
              :style="column.width ? { width: column.width } : undefined"
              @click="column.sortable ? toggleSort(column.key) : undefined"
            >
              <span class="th-inner">
                <span>{{ column.label }}</span>
                <Icon
                  v-if="column.sortable && sortKey === column.key"
                  :name="sortDir === 'asc' ? 'arrow-up' : 'arrow-down'"
                  :size="12"
                />
              </span>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="!pagedRows.length" class="data-table-empty-row">
            <td :colspan="columns.length">
              <EmptyState
                v-if="emptyTitle"
                compact
                :icon="emptyIcon"
                :title="emptyTitle"
                :description="emptyDescription"
              />
              <span v-else class="data-table-empty-text">No records.</span>
            </td>
          </tr>
          <tr
            v-for="(row, index) in pagedRows"
            :key="keyForRow(row, index)"
            :class="rowClasses(row, index)"
            @click="emit('select', row)"
          >
            <td
              v-for="column in columns"
              :key="column.key"
              :class="[
                column.align ? `align-${column.align}` : '',
                column.priority === 'low' ? 'col-low' : '',
              ]"
            >
              <slot :name="`cell-${column.key}`" :row="row" :value="cellValue(row, column)">
                {{ displayCell(cellValue(row, column)) }}
              </slot>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div v-if="pageCount > 1" class="data-table-pager">
      <button class="btn btn-sm" :disabled="page === 0" @click="page = Math.max(0, page - 1)">
        <Icon name="chevron-left" :size="13" />
        <span>Prev</span>
      </button>
      <span class="data-table-page-label">{{ pageLabel }}</span>
      <button
        class="btn btn-sm"
        :disabled="page >= pageCount - 1"
        @click="page = Math.min(pageCount - 1, page + 1)"
      >
        <span>Next</span>
        <Icon name="chevron-right" :size="13" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.data-table {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  gap: 8px;
}

.th-inner {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

th.sortable {
  cursor: pointer;
  user-select: none;
}

th.sortable:hover {
  color: var(--text);
}

.align-right {
  text-align: right;
}

.align-center {
  text-align: center;
}

.data-table-empty-row td {
  padding: 0;
}

.data-table-empty-text {
  display: block;
  color: var(--text-muted);
  text-align: center;
  padding: 14px;
}

.data-table-empty-row:hover td {
  background: transparent;
}

.data-table-pager {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  flex: 0 0 auto;
}

.data-table-page-label {
  color: var(--text-muted);
  font-size: 12px;
}

.data-table-cards {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  flex: 1;
  overflow: auto;
}

.data-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius);
  background: var(--surface);
  padding: 10px 12px;
}

.data-card.selected {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.data-card.danger {
  color: var(--danger-fg);
}

.data-card.success {
  color: var(--success-fg);
}

.data-card.muted {
  color: var(--text-muted);
}

.data-card-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
  min-width: 0;
}

.data-card-label {
  flex: 0 0 auto;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 650;
}

.data-card-value {
  min-width: 0;
  overflow-wrap: anywhere;
  text-align: right;
}
</style>
