import { describe, expect, it } from "vitest";
import type { ProviderMetadata, WorkflowDefinition } from "../../domain/models";
import {
  addDirectTransition,
  applyWorkflowInlineNodeEdit,
  applyWorkflowEdgeEditorDraft,
  applyWorkflowEdgeSemantic,
  asRecord,
  autoArrangeWorkflowLayout,
  createWorkflowNode,
  isSameConnectionPointLoop,
  normalizeWorkflowDefinition,
  removeConditionBranch,
  removeEditableEdge,
  setConditionBranch,
  setWorkflowEdgeHandles,
  setWorkflowEdgeLabelAnchor,
  setWorkflowEdgeLabelOffset,
  uniqueWorkflowNodeId,
  moveWorkflowEdgeEditorDraft,
  recordArray,
  workflowEdgeEditorDraft,
  workflowEdgeOptionId,
  workflowEdgeSemanticOptions,
  workflowNodeSemanticHandles,
  validateWorkflowIssues,
  workflowNodeResultMetadata,
  workflowRunSearchText,
} from "../index";
import { buildGraphEdges, buildGraphNodes } from "../../../ui/adapters/vue-flow/builder";
import { newWorkflowTriggerDraft } from "../editor-defaults";

const WORKFLOW_ID = "00000000-0000-0000-0000-000000000001";
const RUN_ID = "00000000-0000-0000-0000-000000000010";
const NODE_RUN_ID = "00000000-0000-0000-0000-000000000011";
const SEARCH_WORKFLOW_ID = "00000000-0000-0000-0000-000000000034";
const SEARCH_RUN_ID = "00000000-0000-0000-0000-000000000012";

describe("workflow graph utils", () => {
  const workflow: WorkflowDefinition = {
    id: WORKFLOW_ID,
    name: "Flow",
    version: "1.0.0",
    enabled: true,
    input_type: { type: "any" },
    definition: {
      nodes: [
        {
          id: "a",
          kind: "action",
          action: { provider: "Console", function: "run", configuration: {} },
          transitions: { next: { $node: "b" } },
        },
        {
          id: "b",
          kind: "action",
          action: { provider: "Console", function: "run", configuration: {} },
          transitions: {},
        },
      ],
      ui: { layout: { nodes: { a: { x: 10, y: 20 } } } },
    },
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
              action: {
                provider: "Console",
                function: "run",
                timeout_seconds: 60,
                configuration: {},
              },
            },
          ],
        },
      },
      null,
    );

    expect(nodes[0].data.summary).toBe("Console.run");
  });

  it("resolves run result metadata from workflow node action configuration", () => {
    const results = workflowNodeResultMetadata(
      {
        id: "run",
        kind: "action",
        action: { provider: "Console", function: "run", configuration: {} },
        action_name: "Legacy",
        action_function: "ignored",
      },
      [
        {
          name: "Console",
          actions: [
            {
              function_name: "run",
              parameters: [],
              results: [{ name: "stdout", ty: { type: "string" }, label: "Standard Output" }],
            },
          ],
          metadata: { credential_scopes: [], contract: null },
        },
        {
          name: "Legacy",
          actions: [
            {
              function_name: "ignored",
              parameters: [],
              results: [{ name: "legacy", ty: { type: "string" } }],
            },
          ],
          metadata: { credential_scopes: [], contract: null },
        },
      ],
    );

    expect(results).toEqual([{ name: "stdout", ty: { type: "string" }, label: "Standard Output" }]);
  });

  it("builds transition edges", () => {
    expect(buildGraphEdges(workflow)).toMatchObject([
      { source: "a", target: "b", label: "next", type: "workflow" },
    ]);
    expect(buildGraphEdges(workflow)[0].data).toMatchObject({
      kind: "direct",
      transitionKey: "next",
      sourceHandle: "source:direct.next",
      targetHandle: "target:in",
      edgeStyle: "square",
      editable: true,
    });
  });

  it("persists connection handle choices in edge data", () => {
    const draft: WorkflowDefinition = JSON.parse(JSON.stringify(workflow));
    setWorkflowEdgeHandles(draft.definition, "a", "next", "right", "left");
    const edge = buildGraphEdges(draft)[0];
    expect(edge.sourceHandle).toBe("right");
    expect(edge.targetHandle).toBe("left");
    expect(edge.data).toMatchObject({ sourceHandle: "right", targetHandle: "left" });
  });

  it("persists edge style choices in edge data", () => {
    const draft: WorkflowDefinition = JSON.parse(JSON.stringify(workflow));
    setWorkflowEdgeHandles(draft.definition, "a", "next", "right", "left", "bezier");
    let edge = buildGraphEdges(draft)[0];
    expect(edge.type).toBe("workflow");
    expect(edge.data).toMatchObject({ edgeStyle: "bezier" });
    const edgeDraft = workflowEdgeEditorDraft(draft, edge)!;
    edgeDraft.edgeStyle = "straight";
    expect(applyWorkflowEdgeEditorDraft(draft.definition, edge, edgeDraft)).toEqual({
      ok: true,
      semanticKey: "next",
    });
    edge = buildGraphEdges(draft)[0];
    expect(edge.type).toBe("workflow");
    expect(edge.data).toMatchObject({ edgeStyle: "straight" });
  });

  it("persists and clears manual edge label offsets", () => {
    const draft: WorkflowDefinition = JSON.parse(JSON.stringify(workflow));
    setWorkflowEdgeHandles(draft.definition, "a", "next", "right", "left", "bezier");
    let edge = buildGraphEdges(draft)[0];
    setWorkflowEdgeLabelOffset(draft.definition, edge, { x: 24, y: -12 });
    edge = buildGraphEdges(draft)[0];
    expect(edge.data).toMatchObject({
      labelOffset: { x: 24, y: -12 },
      edgeStyle: "bezier",
      sourceHandle: "right",
    });
    // changing handles keeps the manual placement.
    setWorkflowEdgeHandles(draft.definition, "a", "next", "bottom", "top", "square");
    edge = buildGraphEdges(draft)[0];
    expect(edge.data).toMatchObject({ labelOffset: { x: 24, y: -12 } });
    setWorkflowEdgeLabelOffset(draft.definition, edge, null);
    edge = buildGraphEdges(draft)[0];
    expect(edge.data.labelOffset).toBeUndefined();
  });

  it("persists and clears manual edge label anchors", () => {
    const draft: WorkflowDefinition = JSON.parse(JSON.stringify(workflow));
    setWorkflowEdgeHandles(draft.definition, "a", "next", "right", "left", "bezier");
    let edge = buildGraphEdges(draft)[0];
    setWorkflowEdgeLabelAnchor(draft.definition, edge, { position: 0.25 });
    edge = buildGraphEdges(draft)[0];
    expect(edge.data).toMatchObject({
      labelAnchor: { position: 0.25 },
      edgeStyle: "bezier",
      sourceHandle: "right",
    });
    const edgeDraft = workflowEdgeEditorDraft(draft, edge)!;
    expect(edgeDraft.labelAnchor).toBe(25);
    edgeDraft.labelAnchor = 75;
    expect(applyWorkflowEdgeEditorDraft(draft.definition, edge, edgeDraft)).toEqual({
      ok: true,
      semanticKey: "next",
    });
    edge = buildGraphEdges(draft)[0];
    expect(edge.data).toMatchObject({ labelAnchor: { position: 0.75 } });
    setWorkflowEdgeLabelAnchor(draft.definition, edge, null);
    edge = buildGraphEdges(draft)[0];
    expect(edge.data.labelAnchor).toBeUndefined();
  });

  it("generates semantic handles for rich workflow nodes", () => {
    expect(
      workflowNodeSemanticHandles({
        id: "guard",
        kind: "condition",
        transitions: { branches: [{ target: { $node: "end" } }] },
      }),
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: "target:in", type: "target" }),
        expect.objectContaining({
          id: "source:branch.0",
          label: "Condition branch 1",
          semanticOptionId: "branch:0",
        }),
        expect.objectContaining({
          id: "source:branch.new",
          label: "New condition branch",
          semanticOptionId: "branch:new",
        }),
      ]),
    );
    expect(
      workflowNodeSemanticHandles({ id: "route", kind: "switch", parameters: { cases: [{}] } }),
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "source:control.cases.0",
          semanticOptionId: "control:cases:0",
        }),
        expect.objectContaining({
          id: "source:control.default",
          semanticOptionId: "control:default",
        }),
      ]),
    );
    expect(
      workflowNodeSemanticHandles({
        id: "fanout",
        kind: "parallel",
        parameters: { branches: [{ $node: "a" }] },
      }),
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "source:control.branches.0",
          semanticOptionId: "control:branches:0",
        }),
      ]),
    );
    expect(
      workflowNodeSemanticHandles({
        id: "join",
        kind: "join",
        parameters: { wait_for: [{ $node: "a" }] },
      }),
    ).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "source:control.wait_for.0",
          semanticOptionId: "control:wait_for:0",
        }),
      ]),
    );
    expect(workflowNodeSemanticHandles({ id: "guard", kind: "try" })).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ semanticOptionId: "control:body" }),
        expect.objectContaining({ semanticOptionId: "control:catch" }),
      ]),
    );
    expect(workflowNodeSemanticHandles({ id: "batch", kind: "map" })).toEqual(
      expect.arrayContaining([expect.objectContaining({ semanticOptionId: "control:target" })]),
    );
    expect(workflowNodeSemanticHandles({ id: "task", kind: "action" })).toEqual(
      expect.arrayContaining([expect.objectContaining({ semanticOptionId: "direct:next" })]),
    );
  });

  it("rejects only exact same connection point loops", () => {
    expect(
      isSameConnectionPointLoop({
        source: "a",
        target: "a",
        sourceHandle: "top",
        targetHandle: "top",
      }),
    ).toBe(true);
    expect(
      isSameConnectionPointLoop({
        source: "a",
        target: "a",
        sourceHandle: "top",
        targetHandle: "bottom",
      }),
    ).toBe(false);
    expect(
      isSameConnectionPointLoop({
        source: "a",
        target: "b",
        sourceHandle: "top",
        targetHandle: "top",
      }),
    ).toBe(false);
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
              cases: [{ target: { $node: "fanout" } }],
              default: { $node: "done" },
            },
          },
          {
            id: "fanout",
            kind: "parallel",
            parameters: { branches: [{ $node: "a" }, { $node: "b" }] },
          },
          { id: "join", kind: "join", parameters: { wait_for: [{ $node: "a" }, { $node: "b" }] } },
          {
            id: "guard",
            kind: "try",
            parameters: {
              body: { $node: "body" },
              catch: { $node: "recover" },
              finally: { $node: "cleanup" },
            },
          },
          { id: "batch", kind: "map", parameters: { target: { $node: "item" } } },
          {
            id: "race",
            kind: "race",
            parameters: { branches: [{ $node: "fast" }, { $node: "slow" }] },
          },
          { id: "a", kind: "output" },
          { id: "b", kind: "output" },
          { id: "body", kind: "output" },
          { id: "recover", kind: "output" },
          { id: "cleanup", kind: "output" },
          { id: "item", kind: "output" },
          { id: "fast", kind: "output" },
          { id: "slow", kind: "output" },
          { id: "done", kind: "end" },
        ],
      },
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
        expect.objectContaining({ source: "race", target: "fast", label: "race" }),
      ]),
    );
    expect(edges.find((edge) => edge.label === "body")?.data).toMatchObject({
      kind: "control",
      editable: true,
      parameterKey: "body",
    });
  });

  it("creates default nodes for editor palette kinds", () => {
    const nodes: any[] = [{ id: "approval", kind: "approval" }];
    expect(createWorkflowNode("approval", nodes)).toMatchObject({
      id: "approval_2",
      kind: "approval",
      parameters: { approval_type: "generic", prompt: "Approval required" },
      transitions: { on_success: { $node: "end" }, on_reject: { $node: "end" } },
    });
    const conditionNode = createWorkflowNode("condition", nodes);
    expect(recordArray(asRecord(conditionNode.transitions).branches)).toHaveLength(1);
    expect(createWorkflowNode("action", nodes)).toMatchObject({
      kind: "action",
      action: { provider: "", function: "" },
      retry: { max_attempts: 1 },
    });

    for (const kind of [
      "action",
      "approval",
      "loop",
      "condition",
      "wait",
      "switch",
      "toggle",
      "percentage",
      "parallel",
      "join",
      "try",
      "map",
      "race",
      "output",
      "input",
      "subflow",
    ] as const) {
      expect(createWorkflowNode(kind, nodes)).toMatchObject({ kind, retry: { max_attempts: 1 } });
    }

    expect(createWorkflowNode("toggle", nodes)).toMatchObject({
      kind: "toggle",
      parameters: { on: { $node: "end" }, off: { $node: "end" } },
    });
    expect(createWorkflowNode("percentage", nodes)).toMatchObject({
      kind: "percentage",
      parameters: { buckets: [], default: { $node: "end" } },
    });
  });

  it("creates workflow trigger drafts with kind-specific defaults", () => {
    const workflowId = "00000000-0000-0000-0000-000000000042";
    expect(newWorkflowTriggerDraft(workflowId, "cron")).toMatchObject({
      workflow_id: workflowId,
      kind: "cron",
      enabled: true,
      configuration: { cron: "0 * * * *", parameters: {} },
      metadata: {},
    });
    expect(newWorkflowTriggerDraft(workflowId, "manual")).toMatchObject({
      workflow_id: workflowId,
      kind: "manual",
      enabled: true,
      configuration: {},
      metadata: {},
    });
  });

  it("generates stable unique node ids", () => {
    expect(uniqueWorkflowNodeId([{ id: "task" }, { id: "task_2" }], "task")).toBe("task_3");
    expect(uniqueWorkflowNodeId([], "manual approval")).toBe("manual_approval");
  });

  it("adds direct transitions using requested or available keys", () => {
    const node: any = { id: "a", transitions: { next: { $node: "b" } } };
    expect(addDirectTransition(node, "c", "on_failure")).toBe("on_failure");
    expect(node.transitions.on_failure).toEqual({ $node: "c" });
    expect(addDirectTransition(node, "d", "branches")).toBe("on_success");
    expect(node.transitions.on_success).toEqual({ $node: "d" });
  });

  it("offers and applies semantic edge operations for rich nodes", () => {
    const condition: any = { id: "guard", kind: "condition", transitions: {} };
    expect(workflowEdgeSemanticOptions(condition).map((option) => option.id)).toContain(
      "branch:new",
    );
    expect(applyWorkflowEdgeSemantic(condition, "approved", "branch:new")).toBe("branches.0");
    expect(condition.transitions.branches[0]).toEqual({
      when: { value: { $ref: { params: ["value"] } }, equals: true },
      target: { $node: "approved" },
    });

    const route: any = { id: "route", kind: "switch", parameters: { cases: [] } };
    expect(workflowEdgeSemanticOptions(route).map((option) => option.id)).toEqual(
      expect.arrayContaining(["control:cases:new", "control:default"]),
    );
    expect(applyWorkflowEdgeSemantic(route, "fanout", "control:cases:new")).toBe("cases.0");
    expect(applyWorkflowEdgeSemantic(route, "done", "control:default")).toBe("default");
    expect(route.parameters.cases[0]).toMatchObject({ equals: true, target: { $node: "fanout" } });
    expect(route.parameters.default).toEqual({ $node: "done" });
  });

  it("identifies edge semantic option ids", () => {
    expect(
      workflowEdgeOptionId({
        source: "a",
        target: "b",
        data: { kind: "direct", transitionKey: "next", editable: true },
      } as any),
    ).toBe("direct:next");
    expect(
      workflowEdgeOptionId({
        source: "a",
        target: "b",
        data: { kind: "branch", branchIndex: 2, editable: true },
      } as any),
    ).toBe("branch:2");
    expect(
      workflowEdgeOptionId({
        source: "a",
        target: "b",
        data: { kind: "control", parameterKey: "branches", parameterIndex: 1, editable: true },
      } as any),
    ).toBe("control:branches:1");
  });

  it("reads editable details from condition branch and switch case edges", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "guard",
            kind: "condition",
            transitions: {
              branches: [
                {
                  label: "approved",
                  when: { value: { $ref: { params: ["approved"] } }, equals: true },
                  target: { $node: "ok" },
                },
              ],
            },
          },
          {
            id: "route",
            kind: "switch",
            parameters: {
              cases: [{ label: "premium", not_equals: "free", target: { $node: "done" } }],
            },
          },
          { id: "ok", kind: "output" },
          { id: "done", kind: "end" },
        ],
      },
    };
    const edges = buildGraphEdges(rich);
    const branchDraft = workflowEdgeEditorDraft(
      rich,
      edges.find((edge) => edge.source === "guard")!,
    );
    const caseDraft = workflowEdgeEditorDraft(
      rich,
      edges.find((edge) => edge.source === "route")!,
    );

    expect(branchDraft).toMatchObject({
      optionId: "branch:0",
      label: "approved",
      canEditCondition: true,
      canMove: true,
      orderIndex: 0,
      orderCount: 1,
    });
    expect(JSON.parse(branchDraft!.whenJson)).toEqual({
      value: { $ref: { params: ["approved"] } },
      equals: true,
    });
    expect(caseDraft).toMatchObject({
      optionId: "control:cases:0",
      label: "premium",
      matchKind: "not_equals",
      canEditSwitchCase: true,
      canMove: true,
    });
    expect(JSON.parse(caseDraft!.matchJson)).toBe("free");
  });

  it("applies condition branch label, predicate, and target edits", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "guard",
            kind: "condition",
            transitions: {
              branches: [{ when: { equals: true }, target: { $node: "ok" } }],
            },
          },
          { id: "ok", kind: "output" },
          { id: "fail", kind: "end" },
        ],
      },
    };
    const edge = buildGraphEdges(rich).find((item) => item.source === "guard")!;
    const draft = workflowEdgeEditorDraft(rich, edge)!;
    draft.label = "rejected";
    draft.whenJson = JSON.stringify({ value: { $ref: { params: ["approved"] } }, equals: false });
    draft.target = "fail";

    expect(applyWorkflowEdgeEditorDraft(rich.definition, edge, draft)).toEqual({
      ok: true,
      semanticKey: "branches.0",
    });
    expect((rich.definition as any).nodes[0].transitions.branches[0]).toEqual({
      label: "rejected",
      when: { value: { $ref: { params: ["approved"] } }, equals: false },
      target: { $node: "fail" },
    });
  });

  it("applies switch case match edits and default target edits", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "route",
            kind: "switch",
            parameters: {
              cases: [{ equals: "basic", target: { $node: "a" } }],
              default: { $node: "b" },
            },
          },
          { id: "a", kind: "output" },
          { id: "b", kind: "output" },
          { id: "c", kind: "end" },
        ],
      },
    };
    const edges = buildGraphEdges(rich);
    const caseEdge = edges.find((edge) => workflowEdgeOptionId(edge) === "control:cases:0")!;
    const caseDraft = workflowEdgeEditorDraft(rich, caseEdge)!;
    caseDraft.label = "not premium";
    caseDraft.matchKind = "not_equals";
    caseDraft.matchJson = JSON.stringify("premium");
    caseDraft.target = "c";

    expect(applyWorkflowEdgeEditorDraft(rich.definition, caseEdge, caseDraft)).toEqual({
      ok: true,
      semanticKey: "cases.0",
    });
    expect((rich.definition as any).nodes[0].parameters.cases[0]).toEqual({
      label: "not premium",
      not_equals: "premium",
      target: { $node: "c" },
    });

    const defaultEdge = buildGraphEdges(rich).find(
      (edge) => workflowEdgeOptionId(edge) === "control:default",
    )!;
    const defaultDraft = workflowEdgeEditorDraft(rich, defaultEdge)!;
    defaultDraft.target = "c";

    expect(applyWorkflowEdgeEditorDraft(rich.definition, defaultEdge, defaultDraft)).toEqual({
      ok: true,
      semanticKey: "default",
    });
    expect((rich.definition as any).nodes[0].parameters.default).toEqual({ $node: "c" });
  });

  it("moves condition branches and switch cases while preserving edge handle metadata", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "guard",
            kind: "condition",
            transitions: {
              branches: [
                { label: "first", when: { equals: true }, target: { $node: "a" } },
                { label: "second", when: { equals: false }, target: { $node: "b" } },
              ],
            },
          },
          {
            id: "route",
            kind: "switch",
            parameters: {
              cases: [
                { label: "case a", equals: "a", target: { $node: "a" } },
                { label: "case b", equals: "b", target: { $node: "b" } },
              ],
            },
          },
          { id: "a", kind: "output" },
          { id: "b", kind: "end" },
        ],
        ui: {
          edge_handles: {
            "guard:branches.0": { sourceHandle: "left", targetHandle: "right" },
            "guard:branches.1": { sourceHandle: "right", targetHandle: "left" },
            "route:cases.0": { sourceHandle: "top", targetHandle: "bottom" },
            "route:cases.1": { sourceHandle: "bottom", targetHandle: "top" },
          },
        },
      },
    };
    const branchEdge = buildGraphEdges(rich).find(
      (edge) => workflowEdgeOptionId(edge) === "branch:0",
    )!;
    const branchDraft = workflowEdgeEditorDraft(rich, branchEdge)!;
    const caseEdge = buildGraphEdges(rich).find(
      (edge) => workflowEdgeOptionId(edge) === "control:cases:1",
    )!;
    const caseDraft = workflowEdgeEditorDraft(rich, caseEdge)!;

    expect(moveWorkflowEdgeEditorDraft(rich.definition, branchDraft, 1)).toMatchObject({
      ok: true,
      draft: { optionId: "branch:1" },
    });
    expect(
      (rich.definition as any).nodes[0].transitions.branches.map((branch: any) => branch.label),
    ).toEqual(["second", "first"]);
    expect((rich.definition as any).ui.edge_handles["guard:branches.0"]).toEqual({
      sourceHandle: "right",
      targetHandle: "left",
    });
    expect((rich.definition as any).ui.edge_handles["guard:branches.1"]).toEqual({
      sourceHandle: "left",
      targetHandle: "right",
    });

    expect(moveWorkflowEdgeEditorDraft(rich.definition, caseDraft, -1)).toMatchObject({
      ok: true,
      draft: { optionId: "control:cases:0" },
    });
    expect(
      (rich.definition as any).nodes[1].parameters.cases.map((switchCase: any) => switchCase.label),
    ).toEqual(["case b", "case a"]);
    expect((rich.definition as any).ui.edge_handles["route:cases.0"]).toEqual({
      sourceHandle: "bottom",
      targetHandle: "top",
    });
    expect((rich.definition as any).ui.edge_handles["route:cases.1"]).toEqual({
      sourceHandle: "top",
      targetHandle: "bottom",
    });
  });

  it("edits ordered parallel, race, and join target arrays", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "fanout",
            kind: "parallel",
            parameters: { branches: [{ $node: "a" }, { $node: "b" }] },
          },
          { id: "race", kind: "race", parameters: { branches: [{ $node: "a" }, { $node: "b" }] } },
          { id: "join", kind: "join", parameters: { wait_for: [{ $node: "a" }, { $node: "b" }] } },
          { id: "a", kind: "output" },
          { id: "b", kind: "output" },
          { id: "c", kind: "end" },
        ],
      },
    };
    const parallelEdge = buildGraphEdges(rich).find(
      (edge) => edge.source === "fanout" && workflowEdgeOptionId(edge) === "control:branches:0",
    )!;
    const raceEdge = buildGraphEdges(rich).find(
      (edge) => edge.source === "race" && workflowEdgeOptionId(edge) === "control:branches:1",
    )!;
    const joinEdge = buildGraphEdges(rich).find(
      (edge) => edge.source === "join" && workflowEdgeOptionId(edge) === "control:wait_for:0",
    )!;

    const parallelDraft = workflowEdgeEditorDraft(rich, parallelEdge)!;
    parallelDraft.target = "c";
    expect(applyWorkflowEdgeEditorDraft(rich.definition, parallelEdge, parallelDraft)).toEqual({
      ok: true,
      semanticKey: "branches.0",
    });

    const raceDraft = workflowEdgeEditorDraft(rich, raceEdge)!;
    raceDraft.target = "c";
    expect(applyWorkflowEdgeEditorDraft(rich.definition, raceEdge, raceDraft)).toEqual({
      ok: true,
      semanticKey: "branches.1",
    });

    const joinDraft = workflowEdgeEditorDraft(rich, joinEdge)!;
    joinDraft.target = "c";
    expect(applyWorkflowEdgeEditorDraft(rich.definition, joinEdge, joinDraft)).toEqual({
      ok: true,
      semanticKey: "wait_for.0",
    });

    expect((rich.definition as any).nodes[0].parameters.branches).toEqual([
      { $node: "c" },
      { $node: "b" },
    ]);
    expect((rich.definition as any).nodes[1].parameters.branches).toEqual([
      { $node: "a" },
      { $node: "c" },
    ]);
    expect((rich.definition as any).nodes[2].parameters.wait_for).toEqual([
      { $node: "c" },
      { $node: "b" },
    ]);
  });

  it("rejects invalid predicate JSON without mutating the workflow", () => {
    const rich: WorkflowDefinition = {
      ...workflow,
      definition: {
        nodes: [
          {
            id: "guard",
            kind: "condition",
            transitions: {
              branches: [{ when: { equals: true }, target: { $node: "ok" } }],
            },
          },
          { id: "ok", kind: "output" },
          { id: "fail", kind: "end" },
        ],
      },
    };
    const before = JSON.stringify(rich.definition);
    const edge = buildGraphEdges(rich).find((item) => item.source === "guard")!;
    const draft = workflowEdgeEditorDraft(rich, edge)!;
    draft.whenJson = "{";
    draft.target = "fail";

    expect(applyWorkflowEdgeEditorDraft(rich.definition, edge, draft)).toEqual({
      ok: false,
      message: "Condition branch predicate must be valid JSON",
    });
    expect(JSON.stringify(rich.definition)).toBe(before);
  });

  it("maps workflow validation issues to nodes and edges", () => {
    const definition: any = {
      start: "missing_start",
      nodes: [
        {
          id: "task",
          kind: "action",
          action: { provider: "Unknown", function: "run", configuration: {} },
          transitions: { next: { $node: "missing" } },
        },
        { id: "task", kind: "output", parameters: { data: { $ref: { node: "missing" } } } },
        {
          id: "guard",
          kind: "condition",
          transitions: { branches: [{ when: { value: "{{legacy}}" } }] },
        },
        { id: "route", kind: "switch", parameters: { cases: [{ equals: true }] } },
      ],
    };

    const issues = validateWorkflowIssues(definition, [
      { name: "Console", actions: [], metadata: { credential_scopes: [], contract: null } },
    ]);
    expect(issues).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          nodeId: "missing_start",
          message: "Workflow start references missing node missing_start",
        }),
        expect.objectContaining({ nodeId: "task", message: "Duplicate node ID task" }),
        expect.objectContaining({
          nodeId: "task",
          edgeKey: "task:next",
          message: "task.next references missing node missing",
        }),
        expect.objectContaining({ nodeId: "task", message: "task has no outgoing path" }),
        expect.objectContaining({
          nodeId: "task",
          message: "task.parameters.data references missing node missing",
        }),
        expect.objectContaining({ nodeId: "route", message: "route has no outgoing path" }),
        expect.objectContaining({
          nodeId: "guard",
          edgeKey: "guard:branches.0",
          message: 'guard.branches.0 must be { "$node": "node_id" }',
        }),
        expect.objectContaining({
          nodeId: "route",
          edgeKey: "route:cases.0",
          message: 'route.cases.0 must be { "$node": "node_id" }',
        }),
        expect.objectContaining({
          nodeId: "task",
          message: "task references unknown provider Unknown",
        }),
      ]),
    );
  });

  it("treats whitespace-only required action inputs as missing", () => {
    const providers: ProviderMetadata[] = [
      {
        name: "Console",
        actions: [
          {
            function_name: "run",
            parameters: [
              { name: "command", required: true, secret: false, ty: { type: "string" } },
            ],
            results: [],
          },
        ],
        metadata: { credential_scopes: [], contract: null },
      },
    ];
    const blank: any = {
      start: "task",
      nodes: [
        {
          id: "task",
          kind: "action",
          action: { provider: "Console", function: "run", configuration: {} },
          parameters: { command: "   " },
        },
      ],
    };
    expect(validateWorkflowIssues(blank, providers)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ nodeId: "task", message: "task: command is required" }),
      ]),
    );

    const provided: any = {
      start: "task",
      nodes: [
        {
          id: "task",
          kind: "action",
          action: { provider: "Console", function: "run", configuration: {} },
          parameters: { command: "echo hi" },
        },
      ],
    };
    expect(
      validateWorkflowIssues(provided, providers).some((issue) =>
        issue.message.includes("command is required"),
      ),
    ).toBe(false);
  });

  it("applies inline node edits while preserving layout and references", () => {
    const definition: any = {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "approve" } } },
        {
          id: "approve",
          kind: "approval",
          parameters: { prompt: "Old prompt" },
          transitions: { next: { $node: "end" } },
        },
        { id: "end", kind: "end" },
      ],
      ui: {
        layout: { nodes: { approve: { x: 20, y: 40 } } },
        edge_handles: {
          "approve:next": {
            sourceHandle: "right",
            targetHandle: "left",
            labelAnchor: { position: 0.25 },
          },
        },
      },
    };

    expect(applyWorkflowInlineNodeEdit(definition, "approve", "review", "Review Step")).toEqual({
      ok: true,
      nodeId: "review",
    });
    definition.ui.layout.nodes.review = definition.ui.layout.nodes.approve;
    delete definition.ui.layout.nodes.approve;

    // inline edits only rename the node and set its display name; activity stays untouched.
    expect(definition.nodes[1]).toMatchObject({
      id: "review",
      name: "Review Step",
      parameters: { prompt: "Old prompt" },
    });
    expect(definition.nodes[0].transitions.next).toEqual({ $node: "review" });
    expect(definition.ui.layout.nodes.review).toEqual({ x: 20, y: 40 });
    expect(definition.ui.edge_handles["review:next"]).toEqual({
      sourceHandle: "right",
      targetHandle: "left",
      labelAnchor: { position: 0.25 },
    });
    expect(definition.ui.edge_handles["approve:next"]).toBeUndefined();
  });

  it("edits condition branches", () => {
    const node: any = { id: "guard", kind: "condition", transitions: {} };
    setConditionBranch(node, 0, { equals: true }, "ok");
    setConditionBranch(node, 1, { equals: false }, "fail");
    expect(node.transitions.branches).toEqual([
      { when: { equals: true }, target: { $node: "ok" } },
      { when: { equals: false }, target: { $node: "fail" } },
    ]);
    removeConditionBranch(node, 0);
    expect(node.transitions.branches).toEqual([
      { when: { equals: false }, target: { $node: "fail" } },
    ]);
  });

  it("removes editable graph edges without touching control-flow edges", () => {
    const node: any = {
      id: "a",
      transitions: { next: { $node: "b" }, branches: [{ when: {}, target: { $node: "c" } }] },
    };
    expect(
      removeEditableEdge(node, {
        id: "1",
        source: "a",
        target: "b",
        data: { kind: "direct", transitionKey: "next", editable: true },
      } as any),
    ).toBe(true);
    expect(node.transitions.next).toBeUndefined();
    expect(
      removeEditableEdge(node, {
        id: "2",
        source: "a",
        target: "c",
        data: { kind: "branch", branchIndex: 0, editable: true },
      } as any),
    ).toBe(true);
    expect(node.transitions.branches).toEqual([]);
    const controlNode: any = { id: "route", parameters: { target: { $node: "item" } } };
    expect(
      removeEditableEdge(controlNode, {
        id: "3",
        source: "route",
        target: "item",
        data: { kind: "control", editable: false },
      } as any),
    ).toBe(false);
    expect(controlNode.parameters.target).toEqual({ $node: "item" });
  });

  it("normalizes legacy definitions with required start and end nodes", () => {
    const normalized = normalizeWorkflowDefinition(workflow);
    expect(normalized.definition.start).toBe("start");
    expect((normalized.definition as any).nodes.map((node: any) => node.kind)).toEqual([
      "start",
      "action",
      "action",
      "end",
      "fail",
    ]);
    expect(
      (normalized.definition as any).nodes.find((node: any) => node.id === "start").transitions ??
        {},
    ).toEqual({});
    expect(
      (normalized.definition as any).nodes.find((node: any) => node.id === "b").transitions?.next,
    ).toBeUndefined();
    expect((normalized.definition as any).ui.layout.nodes.a).toEqual({ x: 10, y: 20 });
  });

  it("uses legacy layout positions", () => {
    const legacy = {
      ...workflow,
      definition: {
        ...workflow.definition,
        ui: { layout: { a: { x: 30, y: 40 } } },
      },
    };
    expect(buildGraphNodes(legacy, null)[0].position).toEqual({ x: 30, y: 40 });
  });

  it("auto arranges direct workflow nodes from start to end", () => {
    const arranged = autoArrangeWorkflowLayout({
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "task" } } },
        { id: "task", kind: "action", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
      ],
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
        { id: "start", kind: "start", transitions: { next: { $node: "fanout" } } },
        {
          id: "fanout",
          kind: "parallel",
          parameters: { branches: [{ $node: "a" }, { $node: "b" }] },
        },
        { id: "a", kind: "action", transitions: { next: { $node: "join" } } },
        { id: "b", kind: "action", transitions: { next: { $node: "join" } } },
        {
          id: "join",
          kind: "join",
          parameters: { wait_for: [{ $node: "a" }, { $node: "b" }] },
          transitions: { next: { $node: "end" } },
        },
        { id: "end", kind: "end" },
      ],
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
        { id: "start", kind: "start", transitions: { next: { $node: "a" } } },
        { id: "a", kind: "action", transitions: { next: { $node: "b" } } },
        {
          id: "b",
          kind: "action",
          transitions: { next: { $node: "a" }, on_success: { $node: "end" } },
        },
        { id: "end", kind: "end" },
      ],
    });

    expect(arranged.a.x).toBe(arranged.b.x);
    expect(arranged.a.y).not.toBe(arranged.b.y);
    expect(arranged.end.x).toBeGreaterThan(arranged.a.x);
  });

  it("marks the active completed end node as succeeded", () => {
    const normalized = normalizeWorkflowDefinition(workflow);
    const nodes = buildGraphNodes(normalized, {
      run: {
        id: RUN_ID,
        workflow_id: WORKFLOW_ID,
        status: "succeeded",
        active_node_id: "end",
        created_at: "",
        started_at: null,
        finished_at: "",
      },
      nodes: [],
    });
    expect(nodes.find((node) => node.id === "end")?.class).toBe("node-success");
  });

  it("renders output nodes with the output kind instead of fail", () => {
    const nodes = buildGraphNodes(
      {
        id: WORKFLOW_ID,
        name: "output check",
        version: "1.0.0",
        enabled: true,
        input_type: { type: "struct", fields: {} },
        definition: {
          start: "start",
          nodes: [
            { id: "start", kind: "start", transitions: {} },
            {
              id: "output_1",
              kind: "output",
              parameters: { event_type: "workflow.output", data: {} },
              transitions: {},
            },
            { id: "end", kind: "end" },
            { id: "fail", kind: "fail" },
          ],
        },
      },
      null,
    );

    expect(nodes.find((node) => node.id === "output_1")?.data).toMatchObject({
      kind: "output",
      title: "output_1",
    });
    expect(nodes.find((node) => node.id === "fail")?.data).toMatchObject({ kind: "fail" });
  });

  it("marks the active workflow node as running before its node run appears", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: RUN_ID,
        workflow_id: WORKFLOW_ID,
        status: "running",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: null,
      },
      nodes: [
        {
          id: NODE_RUN_ID,
          workflow_run_id: RUN_ID,
          node_id: "a",
          status: "succeeded",
          attempt: 1,
          parameters: {},
          message: null,
        },
      ],
    });
    const active = nodes.find((node) => node.id === "b");
    expect(active?.data.status).toBe("running");
    expect(active?.data.running).toBe(true);
    expect(active?.class).toBe("node-running");
  });

  it("marks the active workflow node as debug paused", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: RUN_ID,
        workflow_id: WORKFLOW_ID,
        status: "debug_paused",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: null,
      },
      nodes: [],
    });

    expect(nodes.find((node) => node.id === "b")?.data.status).toBe("debug_paused");
    expect(nodes.find((node) => node.id === "b")?.class).toBe("node-warning");
  });

  it("renders waiting workflow nodes with the waiting state", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: RUN_ID,
        workflow_id: WORKFLOW_ID,
        status: "waiting",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: null,
      },
      nodes: [
        {
          id: NODE_RUN_ID,
          workflow_run_id: RUN_ID,
          node_id: "b",
          status: "waiting",
          attempt: 1,
          parameters: {},
          message: null,
        },
      ],
    });

    expect(nodes.find((node) => node.id === "b")?.data.status).toBe("waiting");
    expect(nodes.find((node) => node.id === "b")?.class).toBe("node-waiting");
  });

  it("builds workflow run search text with workflow identity", () => {
    expect(
      workflowRunSearchText(
        {
          id: SEARCH_RUN_ID,
          workflow_id: SEARCH_WORKFLOW_ID,
          status: "failed",
          created_at: "",
          started_at: null,
          finished_at: null,
        },
        "Nightly Import",
      ),
    ).toContain("nightly import");
    expect(
      workflowRunSearchText(
        {
          id: SEARCH_RUN_ID,
          workflow_id: SEARCH_WORKFLOW_ID,
          status: "failed",
          created_at: "",
          started_at: null,
          finished_at: null,
        },
        "Nightly Import",
      ),
    ).toContain(SEARCH_WORKFLOW_ID);
  });

  it("marks the active terminal workflow node from the run status", () => {
    const nodes = buildGraphNodes(workflow, {
      run: {
        id: RUN_ID,
        workflow_id: WORKFLOW_ID,
        status: "failed",
        active_node_id: "b",
        created_at: "",
        started_at: null,
        finished_at: "",
      },
      nodes: [],
    });
    expect(nodes.find((node) => node.id === "b")?.class).toBe("node-danger");
  });
});
