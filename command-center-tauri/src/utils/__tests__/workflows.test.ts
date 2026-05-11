import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "../../types/models";
import { buildGraphEdges, buildGraphNodes, normalizeWorkflowDefinition } from "../workflows";

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

  it("normalizes legacy definitions with required start and end nodes", () => {
    const normalized = normalizeWorkflowDefinition(workflow);
    expect(normalized.definition.start).toBe("start");
    expect(normalized.definition.nodes.map((node: any) => node.kind)).toEqual(["start", "task", "task", "end"]);
    expect(normalized.definition.nodes.find((node: any) => node.id === "b").transitions.next).toBe("end");
    expect(normalized.definition.ui.layout.nodes.a).toEqual({ x: 10, y: 20 });
  });

  it("uses legacy layout positions", () => {
    const legacy = {
      ...workflow,
      definition: {
        ...workflow.definition,
        ui: { layout: { a: { x: 30, y: 40 } } }
      }
    };
    expect(buildGraphNodes(legacy, null)[0].position).toEqual({ x: 30, y: 40 });
  });

  it("marks the active completed end node as succeeded", () => {
    const normalized = normalizeWorkflowDefinition(workflow);
    const nodes = buildGraphNodes(normalized, {
      run: {
        id: 10,
        workflow_id: 1,
        status: "succeeded",
        active_node_id: "end",
        created_at: "",
        started_at: null,
        finished_at: ""
      },
      nodes: []
    });
    expect(nodes.find((node) => node.id === "end")?.class).toBe("node-success");
  });
});
