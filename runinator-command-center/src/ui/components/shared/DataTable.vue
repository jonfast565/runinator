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
import LoadingPanel from "./LoadingPanel.vue";
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
    // per-row css classes (e.g. danger/success/muted) matching .table-scroll row variants.
    rowClass?: (row: Row) => string | Record<string, boolean>;
    emptyTitle?: string;
    emptyDescription?: string;
    emptyIcon?: IconName;
    loading?: boolean;
    loadingMessage?: string;
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
    loading: false,
    loadingMessage: undefined,
    initialSortKey: undefined,
    initialSortDir: undefined,
    responsive: "scroll",
  },
);

const { isMobile } = useBreakpoint();
// switch to a stacked card layout on phones when the caller opts in via responsive="cards".
const cardMode = computed(() => props.responsive === "cards" && isMobile.value);

// only blank the table for a full loading panel on the first load (no rows yet). once we have
// rows, a background refresh keeps the current rows mounted and dims them instead of tearing the
// table down and rebuilding it, which reads as a flicker on every poll/event refresh.
const showLoadingPanel = computed(() => props.loading && !props.rows.length);
const refreshing = computed(() => props.loading && props.rows.length > 0);

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

function cardToneClass(row: Row, index: number): string {
  const classes = rowClasses(row, index);
  const tones: string[] = [
    classes.selected ? "border-accent bg-accent-soft" : "border-border-subtle bg-surface",
  ];

  if (classes.danger) {
    tones.push("text-danger-fg");
  }

  if (classes.success) {
    tones.push("text-success-fg");
  }

  if (classes.muted) {
    tones.push("text-fg-muted");
  }

  return tones.join(" ");
}

function alignClass(align?: "left" | "right" | "center"): string {
  if (align === "right") {
    return "text-right";
  }

  if (align === "center") {
    return "text-center";
  }

  return "";
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
  <div v-else class="flex min-h-0 flex-1 flex-col gap-2">
    <LoadingPanel
      v-if="showLoadingPanel"
      compact
      :message="loadingMessage || 'Loading…'"
    />
    <!-- mobile card layout: each row becomes a stack of label:value pairs. -->
    <div
      v-else-if="cardMode"
      class="flex min-h-0 flex-1 flex-col gap-2 overflow-auto"
      :class="refreshing ? 'opacity-60 transition-opacity duration-[120ms]' : ''"
    >
      <EmptyState
        v-if="!pagedRows.length && emptyTitle"
        compact
        :icon="emptyIcon"
        :title="emptyTitle"
        :description="emptyDescription"
      />
      <span v-else-if="!pagedRows.length" class="block px-3.5 py-3.5 text-center text-fg-muted"
        >No records.</span
      >
      <div
        v-for="(row, index) in pagedRows"
        :key="keyForRow(row, index)"
        class="flex flex-col gap-1 rounded-md border px-3 py-2.5"
        :class="cardToneClass(row, index)"
        @click="emit('select', row)"
      >
        <div
          v-for="column in columns"
          :key="column.key"
          class="flex min-w-0 items-baseline justify-between gap-3"
        >
          <span class="shrink-0 text-xs font-semibold text-fg-muted">{{ column.label }}</span>
          <span class="min-w-0 text-right [overflow-wrap:anywhere]" :class="alignClass(column.align)">
            <slot :name="`cell-${column.key}`" :row="row" :value="cellValue(row, column)">
              {{ displayCell(cellValue(row, column)) }}
            </slot>
          </span>
        </div>
      </div>
    </div>
    <div
      v-else
      class="table-scroll"
      :class="refreshing ? 'opacity-60 transition-opacity duration-[120ms]' : ''"
    >
      <table :class="{ compact }">
        <thead>
          <tr>
            <th
              v-for="column in columns"
              :key="column.key"
              :class="[
                alignClass(column.align),
                column.priority === 'low' ? 'col-low' : '',
                column.sortable ? 'cursor-pointer select-none hover:text-fg' : '',
              ]"
              :style="column.width ? { width: column.width } : undefined"
              @click="column.sortable ? toggleSort(column.key) : undefined"
            >
              <span class="inline-flex items-center gap-1">
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
            <td :colspan="columns.length" class="!p-0 hover:!bg-transparent">
              <EmptyState
                v-if="emptyTitle"
                compact
                :icon="emptyIcon"
                :title="emptyTitle"
                :description="emptyDescription"
              />
              <span v-else class="block px-3.5 py-3.5 text-center text-fg-muted">No records.</span>
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
              :class="[alignClass(column.align), column.priority === 'low' ? 'col-low' : '']"
            >
              <slot :name="`cell-${column.key}`" :row="row" :value="cellValue(row, column)">
                {{ displayCell(cellValue(row, column)) }}
              </slot>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div v-if="pageCount > 1" class="flex shrink-0 items-center justify-end gap-2.5">
      <button class="btn btn-sm" :disabled="page === 0" @click="page = Math.max(0, page - 1)">
        <Icon name="chevron-left" :size="13" />
        <span>Prev</span>
      </button>
      <span class="text-xs text-fg-muted">{{ pageLabel }}</span>
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
