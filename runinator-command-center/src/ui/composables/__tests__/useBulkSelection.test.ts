import { describe, expect, it } from "vitest";
import { nextTick, ref } from "vue";
import { useBulkSelection } from "../useBulkSelection";

interface Row {
  id: string;
}

function rowsOf(...ids: string[]): Row[] {
  return ids.map((id) => ({ id }));
}

function setup(ids: string[]) {
  const rows = ref<Row[]>(rowsOf(...ids));
  const selection = useBulkSelection(rows, (row) => row.id);
  return { rows, selection };
}

describe("useBulkSelection", () => {
  it("starts empty", () => {
    const { selection } = setup(["a", "b"]);
    expect(selection.count.value).toBe(0);
    expect(selection.hasSelection.value).toBe(false);
    expect(selection.allSelected.value).toBe(false);
  });

  it("toggles a row on and off", () => {
    const { rows, selection } = setup(["a", "b"]);
    const first = rows.value[0];

    selection.toggle(first);
    expect(selection.isSelected(first)).toBe(true);
    expect(selection.count.value).toBe(1);

    selection.toggle(first);
    expect(selection.isSelected(first)).toBe(false);
  });

  it("selects all and then clears via toggleAll", () => {
    const { selection } = setup(["a", "b", "c"]);

    selection.toggleAll();
    expect(selection.allSelected.value).toBe(true);
    expect(selection.count.value).toBe(3);

    selection.toggleAll();
    expect(selection.count.value).toBe(0);
  });

  it("reports a partial selection as someSelected, not allSelected", () => {
    const { rows, selection } = setup(["a", "b", "c"]);
    selection.toggle(rows.value[1]);

    expect(selection.someSelected.value).toBe(true);
    expect(selection.allSelected.value).toBe(false);
  });

  it("treats an empty list as not all-selected", () => {
    const { selection } = setup([]);
    expect(selection.allSelected.value).toBe(false);
  });

  it("selects an inclusive range on shift-click", () => {
    const { rows, selection } = setup(["a", "b", "c", "d", "e"]);

    selection.toggle(rows.value[1]);
    selection.toggle(rows.value[3], { shiftKey: true } as MouseEvent);

    expect(selection.selectedRows.value.map((row) => row.id)).toEqual(["b", "c", "d"]);
  });

  it("selects a backwards range on shift-click", () => {
    const { rows, selection } = setup(["a", "b", "c", "d"]);

    selection.toggle(rows.value[2]);
    selection.toggle(rows.value[0], { shiftKey: true } as MouseEvent);

    expect(selection.selectedRows.value.map((row) => row.id)).toEqual(["a", "b", "c"]);
  });

  it("returns selected rows in list order regardless of click order", () => {
    const { rows, selection } = setup(["a", "b", "c"]);

    selection.toggle(rows.value[2]);
    selection.toggle(rows.value[0]);

    expect(selection.selectedRows.value.map((row) => row.id)).toEqual(["a", "c"]);
  });

  it("survives a re-sort of the same rows", async () => {
    const { rows, selection } = setup(["a", "b", "c"]);
    selection.toggle(rows.value[0]);

    rows.value = rowsOf("c", "b", "a");
    await nextTick();

    expect(selection.count.value).toBe(1);
    expect(selection.selectedRows.value[0]?.id).toBe("a");
  });

  it("drops keys that leave the visible list", async () => {
    const { rows, selection } = setup(["a", "b", "c"]);
    selection.toggleAll();
    expect(selection.count.value).toBe(3);

    rows.value = rowsOf("a", "b");
    await nextTick();

    expect(selection.count.value).toBe(2);
    expect(selection.selectedKeys.value).toEqual(["a", "b"]);
    expect(selection.allSelected.value).toBe(true);
  });

  it("clears the anchor so a later shift-click does not extend a stale range", () => {
    const { rows, selection } = setup(["a", "b", "c", "d"]);

    selection.toggle(rows.value[0]);
    selection.clear();
    selection.toggle(rows.value[3], { shiftKey: true } as MouseEvent);

    expect(selection.selectedRows.value.map((row) => row.id)).toEqual(["d"]);
  });
});
