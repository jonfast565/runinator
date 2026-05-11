import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "../../types/models";
import { buildGraphEdges, buildGraphNodes } from "../workflows";

describe("workflow graph utils", () => {
  const workflow: WorkflowDefinition = {
    id: 1,
    name: "Flow",
    version: 1,
    enabled: true,
    input_schema: {},
    definition: {
      nodes: [
        { id: "a", kind: "task", task_id: 1, transitions: { next: "b" } },
        { id: "b", kind: "task", task_id: 2, transitions: {} }
      ],
      ui: { layout: { nodes: { a: { x: 10, y: 20 } } } }
    }
  };

  it("builds positioned graph nodes", () => {
    const nodes = buildGraphNodes(workflow, null);
    expect(nodes[0].position).toEqual({ x: 10, y: 20 });
  });

  it("builds transition edges", () => {
    expect(buildGraphEdges(workflow)).toMatchObject([{ source: "a", target: "b", label: "next" }]);
  });
});
