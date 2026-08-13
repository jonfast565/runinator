import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type {
  JsonRecord,
  ProviderMetadata,
  RuninatorType,
  WorkflowRunDetail,
} from "../../domain/models";
import { buildSampleContext, workflowReferenceGroups } from "../workflow-references";
import { setWorkflowCatalogs } from "../../workflow/catalog-registry";
import { testNodeKindCatalog } from "../../workflow/__tests__/catalog-fixtures";

beforeAll(() => {
  setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
});

afterAll(() => {
  setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
});

const inputType: RuninatorType = {
  type: "struct",
  fields: {
    cart: {
      required: true,
      ty: { type: "struct", fields: { total: { required: true, ty: { type: "number" } } } },
    },
    name: { required: true, ty: { type: "string" } },
  },
};

const providers: ProviderMetadata[] = [
  {
    name: "jira",
    actions: [
      {
        function_name: "search",
        parameters: [],
        results: [
          {
            name: "issues",
            ty: {
              type: "array",
              items: {
                type: "struct",
                fields: {
                  key: { required: true, ty: { type: "string" } },
                  fields: {
                    required: true,
                    ty: {
                      type: "struct",
                      fields: {
                        summary: { required: true, ty: { type: "string" } },
                      },
                    },
                  },
                },
              },
            },
          },
          { name: "total", ty: { type: "integer" } },
        ],
      },
    ],
    metadata: { credential_scopes: [] },
  },
];

const nodes: JsonRecord[] = [
  {
    id: "make_ticket",
    kind: "action",
    action: { provider: "jira", function: "search" },
    transitions: { next: { $node: "current" } },
  },
  { id: "current", kind: "action", action: { provider: "jira", function: "search" } },
];

describe("workflowReferenceGroups", () => {
  const groups = workflowReferenceGroups({
    workflowInputType: inputType,
    nodes,
    currentNodeId: "current",
    providers,
  });

  it("flattens workflow parameter fields by dotted path with types", () => {
    const params = groups.find((group) => group.title === "Workflow parameters");
    expect(params).toBeDefined();
    const inserts = params!.references.map((reference) => reference.insert);
    expect(inserts).toContain("params.cart");
    expect(inserts).toContain("params.cart.total");
    expect(inserts).toContain("params.name");
    expect(params!.references.find((r) => r.insert === "params.cart.total")?.type).toBe("number");
  });

  it("groups prior node outputs and excludes the current node", () => {
    const references =
      groups.find((group) => group.title === "Output of make_ticket")?.references ?? [];
    expect(references.map((reference) => reference.insert)).toEqual([
      "make_ticket.issues",
      "make_ticket.issues.0",
      "make_ticket.issues.0.key",
      "make_ticket.issues.0.fields",
      "make_ticket.issues.0.fields.summary",
      "make_ticket.total",
    ]);
    expect(groups.some((group) => group.title === "Output of current")).toBe(false);
  });

  it("always offers the run-state roots", () => {
    const roots = groups.find((group) => group.title === "Run state");
    expect(roots?.references.map((reference) => reference.insert)).toEqual([
      "prev",
      "run",
      "config",
      "secret",
      "interrupt",
    ]);
  });
});

describe("buildSampleContext", () => {
  const detail = {
    run: {
      id: "r1",
      workflow_id: "w1",
      status: "succeeded",
      parameters: { x: 1 },
      created_at: "",
      started_at: null,
      finished_at: null,
    },
    nodes: [
      {
        id: "1",
        workflow_run_id: "r1",
        node_id: "a",
        status: "succeeded",
        attempt: 1,
        parameters: {},
        output_json: { k: "v" },
        message: null,
      },
      {
        id: "2",
        workflow_run_id: "r1",
        node_id: "b",
        status: "succeeded",
        attempt: 1,
        parameters: {},
        output_json: { n: 2 },
        message: null,
      },
    ],
  } as unknown as WorkflowRunDetail;

  it("mirrors the reducer context with params/steps/prev/workflow", () => {
    expect(buildSampleContext(detail)).toMatchObject({
      params: { x: 1 },
      steps: { a: { output: { k: "v" } }, b: { output: { n: 2 } } },
      prev: { n: 2 },
      workflow: { run_id: "r1", workflow_id: "w1", state: "succeeded" },
    });
  });

  it("returns null without a run", () => {
    expect(buildSampleContext(null)).toBeNull();
  });
});

describe("loop-aware references", () => {
  const loopNodes: JsonRecord[] = [
    { id: "seed", kind: "action", action: { provider: "jira", function: "search" }, transitions: { next: { $node: "each" } } },
    { id: "each", kind: "loop", parameters: { items: [] }, transitions: { next: { $node: "body" }, on_success: { $node: "after" } } },
    { id: "body", kind: "transform", transitions: { next: { $node: "each" } } },
    { id: "after", kind: "action", action: { provider: "jira", function: "search" }, transitions: { next: { $node: "future" } } },
    { id: "future", kind: "action", action: { provider: "jira", function: "search" } },
  ];

  it("offers loop state inside the body but not after the exit", () => {
    const inside = workflowReferenceGroups({
      nodes: loopNodes,
      currentNodeId: "body",
      providers,
    });
    const loopRefs = inside.find((group) => group.title === "Output of each")?.references ?? [];
    expect(loopRefs.map((reference) => reference.insert)).toContain("each.item");
    expect(loopRefs.map((reference) => reference.insert)).toContain("each.index");

    const after = workflowReferenceGroups({ nodes: loopNodes, currentNodeId: "after", providers });
    expect(after.some((group) => group.title === "Output of each")).toBe(true);
    expect(
      workflowReferenceGroups({ nodes: loopNodes, currentNodeId: "future", providers }).some(
        (group) => group.title === "Output of future",
      ),
    ).toBe(false);
  });
});
