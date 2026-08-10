import { computed, ref, watch, type ComputedRef, type Ref } from "vue";

export type SelectionKey = string | number;

export interface BulkSelection<Row> {
  // keys currently selected, in no particular order.
  selectedKeys: Ref<SelectionKey[]>;
  // the selected rows, in the order the visible list has them.
  selectedRows: ComputedRef<Row[]>;
  count: ComputedRef<number>;
  hasSelection: ComputedRef<boolean>;
  allSelected: ComputedRef<boolean>;
  someSelected: ComputedRef<boolean>;
  isSelected: (row: Row) => boolean;
  toggle: (row: Row, event?: MouseEvent) => void;
  toggleAll: () => void;
  clear: () => void;
}

// selection state over a reactive list of rows.
//
// selection is keyed, not indexed, so it survives the list re-sorting or refreshing underneath it.
// keys that leave the visible list are dropped: acting on a row the user can no longer see is the
// kind of surprise a bulk action must not produce.
export function useBulkSelection<Row>(
  rows: Ref<Row[]> | ComputedRef<Row[]>,
  keyOf: (row: Row) => SelectionKey,
): BulkSelection<Row> {
  const selectedKeys = ref<SelectionKey[]>([]);
  // anchor for shift-click range selection, held as a key so a re-sort cannot shift the range.
  let anchorKey: SelectionKey | null = null;

  const visibleKeys = computed(() => rows.value.map((row) => keyOf(row)));

  watch(visibleKeys, (keys) => {
    const visible = new Set(keys);
    const pruned = selectedKeys.value.filter((key) => visible.has(key));

    if (pruned.length !== selectedKeys.value.length) {
      selectedKeys.value = pruned;
    }
  });

  const selectedSet = computed(() => new Set(selectedKeys.value));
  const selectedRows = computed(() => rows.value.filter((row) => selectedSet.value.has(keyOf(row))));
  const count = computed(() => selectedRows.value.length);
  const hasSelection = computed(() => count.value > 0);
  const allSelected = computed(
    () => rows.value.length > 0 && count.value === rows.value.length,
  );
  const someSelected = computed(() => hasSelection.value && !allSelected.value);

  function isSelected(row: Row): boolean {
    return selectedSet.value.has(keyOf(row));
  }

  function setSelected(keys: SelectionKey[]) {
    selectedKeys.value = [...new Set(keys)];
  }

  function selectRange(toKey: SelectionKey) {
    const keys = visibleKeys.value;
    const from = anchorKey === null ? -1 : keys.indexOf(anchorKey);
    const to = keys.indexOf(toKey);

    if (from < 0 || to < 0) {
      setSelected([...selectedKeys.value, toKey]);
      return;
    }

    const [start, end] = from <= to ? [from, to] : [to, from];
    setSelected([...selectedKeys.value, ...keys.slice(start, end + 1)]);
  }

  function toggle(row: Row, event?: MouseEvent) {
    const key = keyOf(row);

    if (event?.shiftKey && anchorKey !== null) {
      selectRange(key);
      return;
    }

    anchorKey = key;

    if (selectedSet.value.has(key)) {
      setSelected(selectedKeys.value.filter((candidate) => candidate !== key));
      return;
    }

    setSelected([...selectedKeys.value, key]);
  }

  function toggleAll() {
    anchorKey = null;
    setSelected(allSelected.value ? [] : visibleKeys.value);
  }

  function clear() {
    anchorKey = null;
    selectedKeys.value = [];
  }

  return {
    selectedKeys,
    selectedRows,
    count,
    hasSelection,
    allSelected,
    someSelected,
    isSelected,
    toggle,
    toggleAll,
    clear,
  };
}
