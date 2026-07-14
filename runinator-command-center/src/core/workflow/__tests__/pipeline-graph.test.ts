import { describe, expect, it } from "vitest";
import type { WorkflowDefinition, WorkflowTrigger } from "../../domain/models";
import { buildPipelineGraph } from "../pipeline-graph";

function workflow(id: string, name: string): WorkflowDefinition {
  return {
    id,
    name,
    version: "1.0.0",
    enabled: true,
    input_type: {},
    definition: { nodes: [] },
  };
}

function chainedTrigger(
  id: string,
  workflowId: string,
  target: string,
  on: string,
  enabled = true,
): WorkflowTrigger {
  return {
    id,
    workflow_id: workflowId,
    kind: "chained",
    enabled,
    configuration: { on, target_workflow: target, parameters: {} },
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {},
  };
}

describe("buildPipelineGraph", () => {
  it("maps chained triggers to edges, resolving target names to ids", () => {
    const a = workflow("id-a", "Deploy");
    const b = workflow("id-b", "Smoke Tests");
    const graph = buildPipelineGraph([a, b], {
      "id-a": [chainedTrigger("t1", "id-a", "Smoke Tests", "success")],
      "id-b": [],
    });

    expect(graph.nodes.map((n) => n.id).sort()).toEqual(["id-a", "id-b"]);
    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]).toMatchObject({
      source: "id-a",
      target: "id-b",
      label: "on success",
      data: { triggerId: "t1", on: "success", enabled: true },
    });
    // out/in counts reflect the resolved edge.
    expect(graph.nodes.find((n) => n.id === "id-a")?.data.outgoing).toBe(1);
    expect(graph.nodes.find((n) => n.id === "id-b")?.data.incoming).toBe(1);
    expect(graph.unresolved).toHaveLength(0);
  });

  it("ignores non-chained triggers and normalizes the on-selector", () => {
    const a = workflow("id-a", "Deploy");
    const b = workflow("id-b", "Rollback");
    const cron: WorkflowTrigger = {
      ...chainedTrigger("cronid", "id-a", "", "success"),
      kind: "cron",
      configuration: { cron: "0 * * * *" },
    };
    const graph = buildPipelineGraph([a, b], {
      "id-a": [cron, chainedTrigger("t2", "id-a", "Rollback", "failure")],
      "id-b": [],
    });

    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0].data.on).toBe("failure");
    expect(graph.edges[0].label).toBe("on failure");
  });

  it("flags a chained trigger whose target name does not resolve", () => {
    const a = workflow("id-a", "Deploy");
    const graph = buildPipelineGraph([a], {
      "id-a": [chainedTrigger("t3", "id-a", "Ghost Workflow", "complete", false)],
    });

    expect(graph.edges).toHaveLength(0);
    expect(graph.unresolved).toHaveLength(1);
    expect(graph.unresolved[0]).toMatchObject({
      triggerId: "t3",
      sourceName: "Deploy",
      targetName: "Ghost Workflow",
      on: "complete",
      enabled: false,
    });
  });
});
