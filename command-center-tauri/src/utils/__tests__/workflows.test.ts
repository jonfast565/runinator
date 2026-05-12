import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "../../types/models";
import {
  addDirectTransition,
  buildGraphEdges,
  buildGraphNodes,
  createWorkflowNode,
  normalizeWorkflowDefinition,
  removeConditionBranch,
  removeEditableEdge,
  setConditionBranch,
  uniqueWorkflowNodeId
} from "../workflows";

describe("workflow graph utils", () => {
  const workflow: WorkflowDefinition = {
    id: 1,
    name: "Flow",
    version: 1,
    enabled: true,
    input_schema: {},
    definition: {
      nodes: [
        { id: "a", kind: "task", task_id: 1, transitions: { next: { "$node": "b" } } },
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
    expect(buildGraphEdges(workflow)[0].data).toMatchObject({ kind: "direct", transitionKey: "next", editable: true });
  });

  it("builds rich control-flow parameter edges", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "route",
            kind: "switch",
            parameters: {
              cases: [{ target: { "$node": "fanout" } }],
              default: { "$node": "done" }
            }
          },
          { id: "fanout", kind: "parallel", parameters: { branches: [{ "$node": "a" }, { "$node": "b" }] } },
          { id: "join", kind: "join", parameters: { wait_for: [{ "$node": "a" }, { "$node": "b" }] } },
          { id: "guard", kind: "try", parameters: { body: { "$node": "body" }, catch: { "$node": "recover" }, finally: { "$node": "cleanup" } } },
          { id: "batch", kind: "map", parameters: { target: { "$node": "item" } } },
          { id: "race", kind: "race", parameters: { branches: [{ "$node": "fast" }, { "$node": "slow" }] } },
          { id: "a", kind: "emit" },
          { id: "b", kind: "emit" },
          { id: "body", kind: "emit" },
          { id: "recover", kind: "emit" },
          { id: "cleanup", kind: "emit" },
          { id: "item", kind: "emit" },
          { id: "fast", kind: "emit" },
          { id: "slow", kind: "emit" },
          { id: "done", kind: "end" }
        ]
      }
    };

    const edges = buildGraphEdges(rich);
    expect(edges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ source: "route", target: "fanout", label: "case 1" }),
        expect.objectContaining({ source: "route", target: "done", label: "default" }),
        expect.objectContaining({ source: "fanout", target: "a", label: "branch" }),
        expect.objectContaining({ source: "join", target: "b", label: "wait_for" }),
        expect.objectContaining({ source: "guard", target: "body", label: "body" }),
        expect.objectContaining({ source: "guard", target: "recover", label: "catch" }),
        expect.objectContaining({ source: "guard", target: "cleanup", label: "finally" }),
        expect.objectContaining({ source: "batch", target: "item", label: "target" }),
        expect.objectContaining({ source: "race", target: "fast", label: "race" })
      ])
    );
    expect(edges.find((edge) => edge.label === "body")?.data).toMatchObject({ kind: "control", editable: false, parameterKey: "body" });
  });

  it("creates default nodes for editor palette kinds", () => {
    const nodes: any[] = [{ id: "approval", kind: "approval" }];
    expect(createWorkflowNode("approval", nodes)).toMatchObject({
      id: "approval_2",
      kind: "approval",
      parameters: { approval_type: "generic", prompt: "Approval required" },
      transitions: { on_success: { "$node": "end" }, on_reject: { "$node": "end" } }
    });
    const conditionNode = createWorkflowNode("condition", nodes);
    expect(conditionNode.transitions?.branches).toHaveLength(1);
    expect(createWorkflowNode("task", nodes, 42)).toMatchObject({ kind: "task", task_id: 42, retry: { max_attempts: 1 } });
  });

  it("generates stable unique node ids", () => {
    expect(uniqueWorkflowNodeId([{ id: "task" }, { id: "task_2" }], "task")).toBe("task_3");
    expect(uniqueWorkflowNodeId([], "manual approval")).toBe("manual_approval");
  });

  it("adds direct transitions using requested or available keys", () => {
    const node: any = { id: "a", transitions: { next: { "$node": "b" } } };
    expect(addDirectTransition(node, "c", "on_failure")).toBe("on_failure");
    expect(node.transitions.on_failure).toEqual({ "$node": "c" });
    expect(addDirectTransition(node, "d", "branches")).toBe("on_success");
    expect(node.transitions.on_success).toEqual({ "$node": "d" });
  });

  it("edits condition branches", () => {
    const node: any = { id: "guard", kind: "condition", transitions: {} };
    setConditionBranch(node, 0, { equals: true }, "ok");
    setConditionBranch(node, 1, { equals: false }, "fail");
    expect(node.transitions.branches).toEqual([
      { when: { equals: true }, target: { "$node": "ok" } },
      { when: { equals: false }, target: { "$node": "fail" } }
    ]);
    removeConditionBranch(node, 0);
    expect(node.transitions.branches).toEqual([{ when: { equals: false }, target: { "$node": "fail" } }]);
  });

  it("removes editable graph edges without touching control-flow edges", () => {
    const node: any = { id: "a", transitions: { next: { "$node": "b" }, branches: [{ when: {}, target: { "$node": "c" } }] } };
    expect(removeEditableEdge(node, { id: "1", source: "a", target: "b", data: { kind: "direct", transitionKey: "next", editable: true } } as any)).toBe(true);
    expect(node.transitions.next).toBeUndefined();
    expect(removeEditableEdge(node, { id: "2", source: "a", target: "c", data: { kind: "branch", branchIndex: 0, editable: true } } as any)).toBe(true);
    expect(node.transitions.branches).toEqual([]);
    const controlNode: any = { id: "route", parameters: { target: { "$node": "item" } } };
    expect(removeEditableEdge(controlNode, { id: "3", source: "route", target: "item", data: { kind: "control", editable: false } } as any)).toBe(false);
    expect(controlNode.parameters.target).toEqual({ "$node": "item" });
  });

  it("normalizes legacy definitions with required start and end nodes", () => {
    const normalized = normalizeWorkflowDefinition(workflow);
    expect(normalized.definition.start).toBe("start");
    expect(normalized.definition.nodes.map((node: any) => node.kind)).toEqual(["start", "task", "task", "end"]);
    expect(normalized.definition.nodes.find((node: any) => node.id === "b").transitions.next).toEqual({ "$node": "end" });
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
