import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import type { JsonRecord } from "../../../../core/domain/json";
import type { WorkflowDefinition } from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchWorkflowTriggers: vi.fn(),
  fetchWorkflowRuns: vi.fn(),
  decompileToRexRap: vi.fn(),
}));

import {
  decompileToRexRap,
  fetchWorkflowRuns,
  fetchWorkflowTriggers,
} from "../../../../core/api/commandCenterApi";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { interruptDeclarations } from "../../../../core/workflow/interrupt-regions";
import { testNodeKindCatalog } from "../../../../core/workflow/__tests__/catalog-fixtures";

const WORKFLOW_ID = "00000000-0000-0000-0000-000000000009";

/** a saved workflow carrying all four header declarations. */
function workflow(): WorkflowDefinition {
  return {
    id: WORKFLOW_ID,
    name: "Header",
    version: "1.0.0",
    enabled: true,
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "work" } } },
        { id: "work", kind: "action", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
      ],
      metadata: {
        concurrency: { max_concurrent_runs: 2, on_conflict: "queue" },
        correlation: { $ref: "input.batch_id" },
      },
    },
  } as unknown as WorkflowDefinition;
}

function draftMetadata(): JsonRecord {
  return (useWorkflowsStore().workflowDraft.definition).metadata as JsonRecord;
}

/** one node out of the draft graph, for assertions about what the scaffold actually wrote. */
function draftNode(
  workflows: ReturnType<typeof useWorkflowsStore>,
  id: string,
): JsonRecord | undefined {
  const nodes = workflows.workflowDraft.definition.nodes as JsonRecord[];
  return nodes.find((node) => node.id === id);
}

beforeEach(() => {
  setActivePinia(createPinia());
  setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
  useWorkflowsStore().clearServiceState({ discardDraft: true });
  vi.stubGlobal("window", {
    clearTimeout: () => undefined,
    setTimeout: (fn: () => void) => {
      fn();
      return 0;
    },
  });
  vi.clearAllMocks();
  vi.mocked(decompileToRexRap).mockResolvedValue("workflow stub { start -> end }");
  vi.mocked(fetchWorkflowTriggers).mockResolvedValue([]);
  vi.mocked(fetchWorkflowRuns).mockResolvedValue([]);
});

afterEach(() => {
  setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
});

describe("workflow header draft", () => {
  it("reads the declarations off a selected workflow", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    expect(workflows.headerDraft.concurrency).toEqual({
      maxConcurrentRuns: 2,
      onConflict: "queue",
    });
    expect(workflows.headerDraft.correlation).toEqual({ $ref: "input.batch_id" });
    expect(workflows.headerDraft.interrupts).toEqual([]);
  });

  it("writes an edit through to the definition and the json pane", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    workflows.setHeaderConcurrency({ maxConcurrentRuns: 5, onConflict: "skip" });

    expect(draftMetadata().concurrency).toEqual({ max_concurrent_runs: 5, on_conflict: "skip" });
    expect(workflows.workflowJson).toContain("\"max_concurrent_runs\": 5");
    expect(workflows.isDirty).toBe(true);
  });

  it("removes the key entirely when a section is cleared", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    workflows.clearHeaderConcurrency();

    expect("concurrency" in draftMetadata()).toBe(false);
    // a sibling declaration is left alone.
    expect(draftMetadata().correlation).toEqual({ $ref: "input.batch_id" });
  });
});

describe("scaffoldInterruptHandler", () => {
  it("creates a region that passes validation and declares it", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    expect(workflows.scaffoldInterruptHandler("external")).toBe(true);

    expect(workflows.headerDraft.interrupts).toEqual([
      { source: "external", handler: "on_external", enabled: true },
    ]);
    // the whole point of the scaffold: what it produces is valid the moment it exists.
    expect(workflows.getHeaderIssues()).toEqual([]);
  });

  it("scaffolds an empty route and focuses its first insertion point", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("external");

    // The route starts valid but empty.  Its connection is where the canvas insertion control adds
    // the first handler step, rather than forcing an Audit placeholder into every workflow.
    expect(workflows.getRegionNodeIds("on_external").sort()).toEqual([
      "on_external",
      "resume_external",
    ]);

    const entry = draftNode(workflows, "on_external");

    expect(entry?.kind).toBe("interrupt");
    // metadata links the source; the graph entry stays source-neutral and purely structural.
    expect(entry?.parameters).toBeUndefined();
    expect(entry?.transitions).toEqual({
      next: { $node: "resume_external" },
    });
    expect(workflows.selectedStepId).toBe("");
    expect(workflows.stepEditorOpen).toBe(false);
    expect(workflows.workflowCanvasFocus).toEqual(
      expect.objectContaining({ nodeIds: ["on_external", "resume_external"] }),
    );
    expect(workflows.workflowCanvasFocus.requestId).toBeGreaterThan(0);
  });

  it("keeps source and enabled state in metadata while the graph owns the region", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    workflows.setHeaderInterruptSource(0, "timeout");
    workflows.setHeaderInterruptEnabled(0, false);

    expect(interruptDeclarations(workflows.workflowDraft)).toEqual([
      { source: "timeout", handler: "on_wake", enabled: false },
    ]);
    expect(draftNode(workflows, "on_wake")?.parameters).toBeUndefined();
  });

  it("refuses a second handler for the same source", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("external");

    expect(workflows.scaffoldInterruptHandler("external")).toBe(false);
    expect(workflows.headerDraft.interrupts).toHaveLength(1);
  });

  it("marks the region's nodes on the canvas model", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    const nodes = workflows.graphNodes;
    const entry = nodes.find((node) => node.id === "on_wake");
    const resume = nodes.find((node) => node.id === "resume_wake");
    const main = nodes.find((node) => node.id === "work");
    const route = workflows.graphEdges.find(
      (edge) => edge.source === "on_wake" && edge.target === "resume_wake",
    );

    expect(entry?.data?.interruptRegion).toEqual({
      source: "wake",
      handler: "on_wake",
      enabled: true,
    });
    expect(entry?.data?.interruptEntry).toBe(true);
    expect(resume?.data?.interruptEntry).toBe(false);
    expect(entry?.class).toContain("node-interrupt-region");
    expect(main?.data?.interruptRegion).toBeNull();
    expect(route?.data.interruptRegion).toEqual({
      source: "wake",
      handler: "on_wake",
      enabled: true,
    });
  });

  it("makes a disabled handler visibly inactive on the canvas", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    workflows.setHeaderInterruptEnabled(0, false);

    const region = workflows.graphNodes.filter((node) =>
      ["on_wake", "resume_wake"].includes(node.id),
    );
    expect(region).toHaveLength(2);
    expect(
      region.every(
        (node) => typeof node.class === "string" && node.class.includes("node-interrupt-disabled"),
      ),
    ).toBe(true);
  });

  it("inserts a single-path step into a normal canvas connection", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    const edge = workflows.graphEdges.find((item) => item.source === "work" && item.target === "end");
    const before = new Map(workflows.graphNodes.map((node) => [node.id, node.position]));

    expect(edge).toBeDefined();
    expect(workflows.insertableWorkflowNodeKinds(edge?.id ?? "")).toContain("audit");
    expect(workflows.insertableWorkflowNodeKinds(edge?.id ?? "")).not.toContain("condition");
    expect(workflows.insertWorkflowNodeOnEdge(edge?.id ?? "", "audit")).toBe(true);

    expect(draftNode(workflows, "work")?.transitions).toEqual({ next: { $node: "audit" } });
    expect(draftNode(workflows, "audit")?.transitions).toEqual({ next: { $node: "end" } });
    expect(workflows.selectedStepId).toBe("audit");
    expect(workflows.stepEditorOpen).toBe(true);
    expect(workflows.graphNodes.find((node) => node.id === "audit")?.position).toEqual({
      x: Math.round(((before.get("work")?.x ?? 0) + (before.get("end")?.x ?? 0)) / 2),
      y: Math.round(((before.get("work")?.y ?? 0) + (before.get("end")?.y ?? 0)) / 2),
    });
  });

  it("keeps handler insertion safe and preserves its return to resume", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    const edge = workflows.graphEdges.find(
      (item) => item.source === "on_wake" && item.target === "resume_wake",
    );

    expect(edge).toBeDefined();
    expect(workflows.insertableWorkflowNodeKinds(edge?.id ?? "")).toContain("audit");
    expect(workflows.insertableWorkflowNodeKinds(edge?.id ?? "")).not.toContain("condition");
    expect(workflows.insertWorkflowNodeOnEdge(edge?.id ?? "", "audit")).toBe(true);

    expect(draftNode(workflows, "on_wake")?.transitions).toEqual({ next: { $node: "audit" } });
    expect(draftNode(workflows, "audit")?.transitions).toEqual({ next: { $node: "resume_wake" } });
    expect(workflows.getInterruptIssues()).toEqual([]);
    expect(workflows.insertWorkflowNodeOnEdge(edge?.id ?? "", "condition")).toBe(false);
  });

  it("refuses a canvas edge drawn into the interrupt entry", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    // the backend rejects this too; catching it at the gesture is what keeps the author from
    // discovering it as a validation error after saving.
    const applied = workflows.applyGraphEdgeSemantic(
      { source: "work", target: "on_wake", sourceHandle: null },
      "next",
    );

    expect(applied).toBe(false);
    expect(workflows.getHeaderIssues()).toEqual([]);
  });

  it("locks the entry node's kind without making it undeletable", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");
    workflows.populateStepEditor("on_wake");

    // changing the kind would destroy the declaration, since the node *is* the declaration...
    expect(workflows.selectedStepKindLocked).toBe(true);
    // ...but removing a handler has to be able to delete its whole region, entry included.
    expect(workflows.canRemoveSelectedStep).toBe(true);
  });

  it("keeps the metadata link attached when its graph entry is renamed", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");
    workflows.closeStepEditor();
    workflows.populateStepEditor("on_wake");
    workflows.stepEditor.id = "wake_handler";

    expect(workflows.applyStepEditor()).toBe(true);
    expect(interruptDeclarations(workflows.workflowDraft)).toEqual([
      { source: "wake", handler: "wake_handler", enabled: true },
    ]);
  });

  it("deletes the whole handler region when its entry is removed from the canvas", async () => {
    vi.stubGlobal("confirm", () => true);
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");
    workflows.closeStepEditor();
    workflows.populateStepEditor("on_wake");

    workflows.removeWorkflowStep();

    const ids = workflows.ensureWorkflowNodes().map((node) => node.id);
    expect(ids).not.toEqual(expect.arrayContaining(["on_wake", "resume_wake"]));
    expect(interruptDeclarations(workflows.workflowDraft)).toEqual([]);
  });

});

describe("the interrupts panel", () => {
  it("opens as its own inspector mode and refreshes the draft", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());

    workflows.openWorkflowInterrupts();

    expect(workflows.workflowInspectorMode).toBe("interrupts");
    // it reads the same draft the header panel does; only the panel differs.
    expect(workflows.headerDraft.interrupts).toEqual([]);
  });

  it("badges each panel with only the issues that panel can fix", async () => {
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.workflowDraft.definition.metadata = {
      interrupts: [{ on: "wake", handler: "does_not_exist" }],
    };
    workflows.openWorkflowInterrupts();

    expect(workflows.getInterruptIssues()).not.toEqual([]);
    expect(workflows.getDeclarationIssues()).toEqual([]);
  });
});

describe("removeHeaderInterrupt", () => {
  it("leaves the handler intact when the user cancels deletion", async () => {
    vi.stubGlobal("confirm", () => false);
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    workflows.removeHeaderInterrupt(0);

    expect(workflows.headerDraft.interrupts).toEqual([
      { source: "wake", handler: "on_wake", enabled: true },
    ]);
    const ids = workflows.ensureWorkflowNodes().map((node) => node.id);
    expect(ids).toContain("on_wake");
    expect(ids).toContain("resume_wake");
  });

  it("deletes the region's nodes when the user confirms cleanup", async () => {
    vi.stubGlobal("confirm", () => true);
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    workflows.removeHeaderInterrupt(0);

    const ids = workflows.ensureWorkflowNodes().map((node) => node.id);
    expect(ids).not.toContain("on_wake");
    expect(ids).not.toContain("resume_wake");
    // the main flow is untouched.
    expect(ids).toEqual(expect.arrayContaining(["start", "work", "end"]));
    expect(workflows.headerDraft.interrupts).toEqual([]);
    expect(draftMetadata().interrupts).toBeUndefined();
  });

  it("does not prompt when the declaration's handler has no region to clean up", async () => {
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal("confirm", confirmSpy);
    const workflows = useWorkflowsStore();
    await workflows.selectWorkflow(workflow());
    workflows.workflowDraft.definition.metadata = {
      interrupts: [{ on: "wake", handler: "does_not_exist" }],
    };
    workflows.openWorkflowInterrupts();

    workflows.removeHeaderInterrupt(0);

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(workflows.headerDraft.interrupts).toEqual([]);
  });
});
