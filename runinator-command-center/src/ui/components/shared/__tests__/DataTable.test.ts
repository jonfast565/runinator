import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import DataTable, { type DataTableColumn } from "../DataTable.vue";

interface Row {
  id: number;
  name: string;
}

const columns: DataTableColumn<Row>[] = [
  { key: "id", label: "ID", sortable: true },
  { key: "name", label: "Name", sortable: true },
];

const rows: Row[] = [
  { id: 1, name: "alpha" },
  { id: 2, name: "bravo" },
  { id: 3, name: "charlie" },
  { id: 4, name: "delta" },
  { id: 5, name: "echo" },
];

function render(props: Record<string, unknown>) {
  return renderToString(createSSRApp({ render: () => h(DataTable, props) }));
}

describe("DataTable (column mode)", () => {
  it("renders column headers and cell values", async () => {
    const html = await render({ columns, rows });
    expect(html).toContain("ID");
    expect(html).toContain("Name");
    expect(html).toContain("alpha");
  });

  it("paginates to the first page and shows a pager", async () => {
    const html = await render({ columns, rows, rowKey: "id", pageSize: 2 });
    expect(html).toContain("alpha");
    expect(html).toContain("bravo");
    expect(html).not.toContain("charlie");
    expect(html).toContain("Page 1 of 3");
  });

  it("applies the initial descending sort", async () => {
    const html = await render({
      columns,
      rows,
      rowKey: "id",
      pageSize: 2,
      initialSortKey: "id",
      initialSortDir: "desc",
    });
    // desc by id => echo (5), delta (4) on the first page; alpha (1) excluded.
    expect(html).toContain("echo");
    expect(html).toContain("delta");
    expect(html).not.toContain("alpha");
  });

  it("shows the empty state when there are no rows", async () => {
    const html = await render({
      columns,
      rows: [],
      emptyTitle: "No records yet",
      emptyDescription: "Nothing here.",
    });
    expect(html).toContain("No records yet");
    expect(html).toContain('aria-label="About this empty state"');
    expect(html).not.toContain("Nothing here.");
  });

  it("owns the table element when no columns are given", async () => {
    const html = await renderToString(
      createSSRApp({ render: () => h(DataTable, {}, { default: () => h("tbody", "custom") }) }),
    );
    expect(html).toContain("table-scroll");
    expect(html.match(/<table/g)).toHaveLength(1);
    expect(html).toContain("custom");
    expect(html).toContain("data-table-mobile-columns-2");
  });

  it("retains the requested leading columns on phones", async () => {
    const html = await render({ columns, rows, mobileColumns: 1 });

    expect(html).toContain("data-table-mobile-columns-1");
  });

  it("supports an explicit mobile column selection when source order is not suitable", async () => {
    const html = await render({
      columns: [
        { key: "id", label: "ID" },
        { key: "name", label: "Name", mobile: true },
      ],
      rows,
    });

    expect(html).toContain("data-table-mobile-columns-explicit");
    expect(html).toContain("col-mobile-hidden");
  });

  it("renders a table skeleton, not rows, on a first load", async () => {
    const html = await render({
      columns,
      rows: [],
      loading: true,
      loadingMessage: "Loading rows…",
    });
    expect(html).toContain("Loading rows…");
    expect(html).toContain("animate-pulse");
  });

  it("keeps rows mounted and dimmed while refreshing", async () => {
    const html = await render({ columns, rows, rowKey: "id", loading: true });
    expect(html).toContain("alpha");
    expect(html).toContain("opacity-60");
    expect(html).not.toContain("animate-pulse");
  });
});

describe("DataTable (selection)", () => {
  it("renders no checkbox column unless selectable", async () => {
    const html = await render({ columns, rows, rowKey: "id" });
    expect(html).not.toContain('type="checkbox"');
  });

  it("renders a header checkbox and one per row", async () => {
    const html = await render({ columns, rows, rowKey: "id", selectable: true });
    // one header + five rows.
    expect(html.match(/type="checkbox"/g)).toHaveLength(6);
    expect(html).toContain('aria-label="Select all"');
  });

  it("labels each row checkbox from the first column and the caller's noun", async () => {
    const html = await render({
      columns: [
        { key: "name", label: "Name" },
        { key: "id", label: "ID" },
      ],
      rows,
      rowKey: "id",
      selectable: true,
      selectionNoun: "workflow",
    });
    expect(html).toContain('aria-label="Select workflow alpha"');
  });

  it("checks exactly the rows named in selectedKeys", async () => {
    const html = await render({
      columns,
      rows,
      rowKey: "id",
      selectable: true,
      selectedKeys: [1, 3],
    });
    expect(html.match(/<input[^>]*\bchecked\b[^>]*>/g)).toHaveLength(2);
  });

  it("flips the header label once everything is selected", async () => {
    const html = await render({
      columns,
      rows,
      rowKey: "id",
      selectable: true,
      selectedKeys: [1, 2, 3, 4, 5],
      allSelected: true,
    });
    expect(html).toContain('aria-label="Deselect all"');
  });

  it("spans the checkbox column in the empty row so the empty state stays centered", async () => {
    const html = await render({
      columns,
      rows: [],
      selectable: true,
      emptyTitle: "No records yet",
    });
    expect(html).toContain('colspan="3"');
  });
});
