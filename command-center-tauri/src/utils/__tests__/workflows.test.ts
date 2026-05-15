import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "../../types/models";
import {
  addDirectTransition,
  autoArrangeWorkflowLayout,
  buildGraphEdges,
  buildGraphNodes,
  copyWorkflowTaskDraft,
  createWorkflowTaskDraft,
  createWorkflowNode,
  isSameConnectionPointLoop,
  normalizeWorkflowDefinition,
  removeConditionBranch,
  removeEditableEdge,
  setConditionBranch,
  setWorkflowEdgeHandles,
  stampWorkflowTaskConfiguration,
  uniqueWorkflowNodeId,
  workflowRunSearchText
} from "../workflows";
import { newWorkflowTriggerDraft } from "../../stores/workflows";

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

  it("does not add status classes without run detail", () => {
    const nodes = buildGraphNodes(workflow, null);
    expect(nodes.every((node) => node.class === "")).toBe(true);
    expect(nodes.every((node) => !node.data.status)).toBe(true);
  });

  it("summarizes imported action nodes from embedded action configuration", () => {
    const nodes = buildGraphNodes(
      {
        ...workflow,
        definition: {
          nodes: [
            {
              id: "run",
              kind: "action",
              action: { provider: "Console", function: "run", timeout_seconds: 60, configuration: {} }
            }
          ]
        }
      },
      null
    );

    expect(nodes[0].data.summary).toBe("Action: Console.run");
  });

  it("builds transition edges", () => {
    expect(buildGraphEdges(workflow)).toMatchObject([{ source: "a", target: "b", label: "next" }]);
    expect(buildGraphEdges(workflow)[0].data).toMatchObject({ kind: "direct", transitionKey: "next", sourceHandle: "bottom", targetHandle: "top", editable: true });
  });

  it("persists connection handle choices in edge data", () => {
    const draft: WorkflowDefinition = JSON.parse(JSON.stringify(workflow));
    setWorkflowEdgeHandles(draft.definition, "a", "next", "right", "left");
    const edge = buildGraphEdges(draft)[0];
    expect(edge.sourceHandle).toBe("right");
    expect(edge.targetHandle).toBe("left");
    expect(edge.data).toMatchObject({ sourceHandle: "right", targetHandle: "left" });
  });

  it("rejects only exact same connection point loops", () => {
    expect(isSameConnectionPointLoop({ source: "a", target: "a", sourceHandle: "top", targetHandle: "top" })).toBe(true);
    expect(isSameConnectionPointLoop({ source: "a", target: "a", sourceHandle: "top", targetHandle: "bottom" })).toBe(false);
    expect(isSameConnectionPointLoop({ source: "a", target: "b", sourceHandle: "top", targetHandle: "top" })).toBe(false);
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
    for (const kind of ["task", "approval", "loop", "condition", "wait", "switch", "parallel", "join", "try", "map", "race", "emit", "subflow"] as const) {
      expect(createWorkflowNode(kind, nodes)).toMatchObject({ kind });
    }
  });

  it("creates and copies workflow-owned task drafts", () => {
    const draft = createWorkflowTaskDraft("build_step", -1);
    expect(draft).toMatchObject({
      id: -1,
      name: "Build Step Task",
      enabled: false,
      configuration: { task_type: "workflow", workflow_node_id: "build_step" }
    });

    const copy = copyWorkflowTaskDraft(
      {
        ...draft,
        id: 42,
        name: "Shared Task",
        action_name: "console",
        action_function: "run",
        configuration: { task_type: "scheduled" }
      },
      "copied",
      -2
    );
    expect(copy).toMatchObject({
      id: -2,
      name: "Shared Task copy",
      action_name: "console",
      action_function: "run",
      configuration: { task_type: "workflow", workflow_node_id: "copied" }
    });
  });

  it("stamps workflow id on owned task configuration", () => {
    const task = createWorkflowTaskDraft("node", -1);
    expect(stampWorkflowTaskConfiguration(task, "renamed", 99).configuration).toMatchObject({
      task_type: "workflow",
      workflow_node_id: "renamed",
      workflow_id: 99
    });
  });

  it("creates workflow trigger drafts with kind-specific defaults", () => {
    expect(newWorkflowTriggerDraft(42, "cron")).toMatchObject({
      workflow_id: 42,
      kind: "cron",
      enabled: true,
      configuration: { cron: "0 * * * *", parameters: {} },
      metadata: {}
    });
    expect(newWorkflowTriggerDraft(42, "manual")).toMatchObject({
      workflow_id: 42,
      kind: "manual",
      enabled: true,
      configuration: {},
      metadata: {}
    });
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
    expect(normalized.definition.nodes.map((node: any) => node.kind)).toEqual(["start", "task", "task", "end", "fail"]);
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

  it("auto arranges direct workflow nodes from start to end", () => {
    const arranged = autoArrangeWorkflowLayout({
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { "$node": "task" } } },
        { id: "task", kind: "task", transitions: { next: { "$node": "end" } } },
        { id: "end", kind: "end" }
      ]
    });

    expect(arranged.start.x).toBeLessThan(arranged.task.x);
    expect(arranged.task.x).toBeLessThan(arranged.end.x);
    expect(arranged.start.y).toBe(arranged.task.y);
    expect(arranged.task.y).toBe(arranged.end.y);
  });

  it("auto arranges branches on the same rank before their join", () => {
    const arranged = autoArrangeWorkflowLayout({
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { "$node": "fanout" } } },
        { id: "fanout", kind: "parallel", parameters: { branches: [{ "$node": "a" }, { "$node": "b" }] } },
        { id: "a", kind: "task", transitions: { next: { "$node": "join" } } },
        { id: "b", kind: "task", transitions: { next: { "$node": "join" } } },
        { id: "join", kind: "join", parameters: { wait_for: [{ "$node": "a" }, { "$node": "b" }] }, transitions: { next: { "$node": "end" } } },
        { id: "end", kind: "end" }
      ]
    });

    expect(arranged.a.x).toBe(arranged.b.x);
    expect(arranged.a.y).not.toBe(arranged.b.y);
    expect(arranged.join.x).toBeGreaterThan(arranged.a.x);
    expect(arranged.end.x).toBeGreaterThan(arranged.join.x);
  });

  it("auto arranges cyclic nodes without recursive rank growth", () => {
    const arranged = autoArrangeWorkflowLayout({
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { "$node": "a" } } },
        { id: "a", kind: "task", transitions: { next: { "$node": "b" } } },
        { id: "b", kind: "task", transitions: { next: { "$node": "a" }, on_success: { "$node": "end" } } },
        { id: "end", kind: "end" }
      ]
    });

    expect(arranged.a.x).toBe(arranged.b.x);
    expect(arranged.a.y).not.toBe(arranged.b.y);
    expect(arranged.end.x).toBeGreaterThan(arranged.a.x);
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

  it("marks the active workflow node as running before its node run appears", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: 10,
        workflow_id: 1,
        status: "running",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: null
      },
      nodes: [
        {
          id: 1,
          workflow_run_id: 10,
          node_id: "a",
          task_run_id: 20,
          status: "succeeded",
          attempt: 1,
          parameters: {},
          message: null
        }
      ]
    });
    const active = nodes.find((node) => node.id === "b");
    expect(active?.data.status).toBe("running");
    expect(active?.data.running).toBe(true);
    expect(active?.class).toBe("node-running");
  });

  it("marks the active workflow node as debug paused", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: 10,
        workflow_id: 1,
        status: "debug_paused",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: null
      },
      nodes: []
    });

    expect(nodes.find((node) => node.id === "b")?.data.status).toBe("debug_paused");
    expect(nodes.find((node) => node.id === "b")?.class).toBe("node-warning");
  });

  it("builds workflow run search text with workflow identity", () => {
    expect(workflowRunSearchText({
      id: 12,
      workflow_id: 34,
      status: "failed",
      created_at: "",
      started_at: null,
      finished_at: null
    }, "Nightly Import")).toContain("nightly import");
    expect(workflowRunSearchText({
      id: 12,
      workflow_id: 34,
      status: "failed",
      created_at: "",
      started_at: null,
      finished_at: null
    }, "Nightly Import")).toContain("34");
  });

  it("marks the active terminal workflow node from the run status", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: 10,
        workflow_id: 1,
        status: "failed",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: ""
      },
      nodes: []
    });
    expect(nodes.find((node) => node.id === "b")?.class).toBe("node-danger");
  });
});
