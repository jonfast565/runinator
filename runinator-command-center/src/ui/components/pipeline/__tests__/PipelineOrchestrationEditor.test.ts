import { describe, expect, it } from "vitest";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import type { Pipeline } from "../../../../core/domain/models";
import { defaultPipelineDefaults } from "../../../../core/domain/models";
import PipelineOrchestrationEditor from "../PipelineOrchestrationEditor.vue";

function pipelineWithOmittedPredicates(): Pipeline {
  return {
    id: "44ba5859-0be2-4184-b9ae-c760529143d7",
    name: "Ticket-scoped Autonomous Development",
    description: null,
    graph: {
      version: 1,
      members: [
        {
          key: "runinator.autodev.plan",
          workflow_id: "c2426b80-f071-4dff-802c-b838937113c7",
          failure_mode: "continue",
        },
      ],
      links: [],
      joins: {},
    },
    concurrency: { max_concurrent_runs: 0, on_conflict: "allow" },
    defaults: defaultPipelineDefaults(),
    metadata: {
      ingress: {
        scope: "ticket.lifecycle",
        // `predicates` is omitted when the Rust Vec is empty.
        routes: [{ event_type: "ready", lifecycle: "unbound", action: "start" }],
      },
      orchestration: { intents: {}, phases: {}, budgets: {}, defaults: null },
    },
  };
}

describe("PipelineOrchestrationEditor", () => {
  it("renders routes whose empty predicate list was omitted on the wire", async () => {
    const html = await renderToString(
      createSSRApp({
        render: () =>
          h(PipelineOrchestrationEditor, {
            pipeline: pipelineWithOmittedPredicates(),
            adapterKinds: [],
          }),
      }),
    );

    expect(html).toContain("Orchestration enabled");
    expect(html).toContain("ready");
  });
});
