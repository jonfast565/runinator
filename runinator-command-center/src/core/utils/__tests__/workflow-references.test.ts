import { describe, expect, it } from "vitest";
import type {
  JsonRecord,
  ProviderMetadata,
  RuninatorType,
  WorkflowRunDetail,
} from "../../domain/models";
import { buildSampleContext, workflowReferenceGroups } from "../workflow-references";

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
  { id: "make_ticket", kind: "action", action: { provider: "jira", function: "search" } },
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
