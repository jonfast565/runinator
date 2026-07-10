import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { getAtLocation } from "../../../../core/workflow/field-location";
import type { WorkflowDefinition, WorkflowNodeKindMetadata } from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  decompileToWdl: vi.fn(),
}));

import { decompileToWdl } from "../../../../core/api/commandCenterApi";

const WORKFLOW_ID = "00000000-0000-0000-0000-000000000099";

const waitMeta: WorkflowNodeKindMetadata = {
  kind: "wait",
  label: "Wait",
  icon: "clock",
  description: "Pauses the run.",
  category: "control-flow",
  protected: false,
  terminal: false,
  addable: true,
  supports_predicate_edges: true,
  fields: [
    {
      name: "seconds",
      ty: { type: "duration" },
      required: false,
      secret: false,
      location: { base: "wait", path: ["seconds"] },
      widget: "duration",
    },
  ],
  edge_slots: [],
  default_template: {
    kind: "wait",
    wait: { seconds: 60 },
    parameters: {},
    retry: { max_attempts: 1 },
    transitions: {},
  },
};

const mutexMeta: WorkflowNodeKindMetadata = {
  kind: "mutex",
  label: "Mutex",
  icon: "lock",
  description: "Acquires a mutex.",
  category: "sync",
  protected: false,
  terminal: false,
  addable: true,
  supports_predicate_edges: true,
  fields: [
    {
      name: "name",
      ty: { type: "string" },
      required: true,
      secret: false,
      location: { base: "parameters", path: ["name"] },
    },
  ],
  edge_slots: [],
  default_template: {
    kind: "mutex",
    parameters: { name: "my-mutex" },
    retry: { max_attempts: 1 },
    transitions: { on_success: { $node: "end" }, on_failure: { $node: "end" } },
  },
};

function sampleWorkflow(): WorkflowDefinition {
  return {
    id: WORKFLOW_ID,
    name: "Catalog Editor",
    version: "1.0.0",
    enabled: true,
    input_type: { type: "any" },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "wait_1" } } },
        {
          id: "wait_1",
          kind: "wait",
          wait: { seconds: 60 },
          parameters: {},
          retry: { max_attempts: 1 },
          transitions: { next: { $node: "end" } },
        },
        {
          id: "mutex_1",
          kind: "mutex",
          parameters: { name: "my-mutex" },
          retry: { max_attempts: 1 },
          transitions: { on_success: { $node: "end" }, on_failure: { $node: "end" } },
        },
        { id: "end", kind: "end" },
      ],
      ui: { layout: { nodes: {} } },
    },
  };
}

describe("catalog-driven step editor round trips", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    setWorkflowCatalogs({
      nodeKinds: [waitMeta, mutexMeta],
      triggerKinds: [],
      enums: [],
    });
    vi.mocked(decompileToWdl).mockResolvedValue("workflow Catalog {}");
  });

  afterEach(() => {
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
  });

  it("populate → mutate wait field → apply preserves node shape", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(sampleWorkflow());

    workflows.populateStepEditor("wait_1");
    expect(workflows.stepEditor.kind).toBe("wait");
    expect(getAtLocation(workflows.stepEditor.nodeDraft, waitMeta.fields[0]!.location)).toBe(60);

    workflows.stepEditor.nodeDraft = {
      ...workflows.stepEditor.nodeDraft,
      wait: { seconds: 120 },
    };

    expect(workflows.applyStepEditor()).toBe(true);

    const saved = workflows.ensureWorkflowNodes().find((node) => node.id === "wait_1") as {
      wait?: { seconds?: number };
    };
    expect(saved.wait?.seconds).toBe(120);
  });

  it("populate → mutate mutex name → apply preserves transitions", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(sampleWorkflow());

    workflows.populateStepEditor("mutex_1");
    workflows.stepEditor.nodeDraft = {
      ...workflows.stepEditor.nodeDraft,
      parameters: { name: "other-mutex" },
    };

    expect(workflows.applyStepEditor()).toBe(true);

    const saved = workflows.ensureWorkflowNodes().find((node) => node.id === "mutex_1") as {
      parameters?: { name?: string };
      transitions?: Record<string, unknown>;
    };
    expect(saved.parameters?.name).toBe("other-mutex");
    expect(saved.transitions).toMatchObject({
      on_success: { $node: "end" },
      on_failure: { $node: "end" },
    });
  });
});
