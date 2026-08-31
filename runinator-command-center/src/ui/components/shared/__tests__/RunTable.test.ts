import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import DataTable from "../DataTable.vue";
import RunTable from "../RunTable.vue";

describe("RunTable", () => {
  it("renders the workflow API trigger source", async () => {
    const app = createSSRApp({
      render: () =>
        h(RunTable, {
          runs: [
            {
              id: "run-1",
              workflow_id: "workflow-1",
              status: "succeeded",
              trigger_source_kind: "cron",
              created_at: "2026-08-31T12:00:00Z",
              started_at: "2026-08-31T12:00:01Z",
              finished_at: "2026-08-31T12:00:02Z",
            },
          ],
          selectedRunId: null,
        }),
    });
    app.component("DataTable", DataTable);
    const html = await renderToString(app);

    expect(html).toContain("Trigger");
    expect(html).toContain("cron");
  });
});
