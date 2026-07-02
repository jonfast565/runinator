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
    expect(html).toContain("Nothing here.");
  });

  it("stays a plain scroll wrapper when no columns are given", async () => {
    const html = await renderToString(
      createSSRApp({ render: () => h(DataTable, {}, { default: () => h("table", "custom") }) }),
    );
    expect(html).toContain("table-scroll");
    expect(html).toContain("custom");
  });
});
