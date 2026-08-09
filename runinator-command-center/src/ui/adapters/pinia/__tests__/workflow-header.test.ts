import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import type { JsonRecord } from "../../../../core/domain/json";
import type { WorkflowDefinition } from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  fetchWorkflowTriggers: vi.fn(),
  fetchWorkflowRuns: vi.fn(),
  decompileToWdl: vi.fn(),
}));

import {
  decompileToWdl,
  fetchWorkflowRuns,
  fetchWorkflowTriggers,
} from "../../../../core/api/commandCenterApi";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
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
  vi.mocked(decompileToWdl).mockResolvedValue("workflow stub { start -> end }");
  vi.mocked(fetchWorkflowTriggers).mockResolvedValue([]);
  vi.mocked(fetchWorkflowRuns).mockResolvedValue([]);
});

afterEach(() => {
  setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
});

describe("workflow header draft", () => {
  it("reads the declarations off a selected workflow", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());

    expect(workflows.headerDraft.concurrency).toEqual({
      maxConcurrentRuns: 2,
      onConflict: "queue",
    });
    expect(workflows.headerDraft.correlation).toEqual({ $ref: "input.batch_id" });
    expect(workflows.headerDraft.interrupts).toEqual([]);
  });

  it("writes an edit through to the definition and the json pane", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());

    workflows.setHeaderConcurrency({ maxConcurrentRuns: 5, onConflict: "skip" });

    expect(draftMetadata().concurrency).toEqual({ max_concurrent_runs: 5, on_conflict: "skip" });
    expect(workflows.workflowJson).toContain("\"max_concurrent_runs\": 5");
    expect(workflows.isDirty).toBe(true);
  });

  it("removes the key entirely when a section is cleared", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());

    workflows.clearHeaderConcurrency();

    expect("concurrency" in draftMetadata()).toBe(false);
    // a sibling declaration is left alone.
    expect(draftMetadata().correlation).toEqual({ $ref: "input.batch_id" });
  });
});

describe("scaffoldInterruptHandler", () => {
  it("creates a region that passes validation and declares it", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());

    expect(workflows.scaffoldInterruptHandler("external")).toBe(true);

    expect(workflows.headerDraft.interrupts).toEqual([
      { source: "external", handler: "on_external" },
    ]);
    // the whole point of the scaffold: what it produces is valid the moment it exists.
    expect(workflows.getHeaderIssues()).toEqual([]);
  });

  it("strips the audit template's edge to `end`, which would drag end into the region", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("external");

    expect(workflows.getRegionNodeIds("on_external").sort()).toEqual([
      "on_external",
      "resume_external",
    ]);
  });

  it("refuses a second handler for the same source", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("external");

    expect(workflows.scaffoldInterruptHandler("external")).toBe(false);
    expect(workflows.headerDraft.interrupts).toHaveLength(1);
  });

  it("marks the region's nodes on the canvas model", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    const nodes = workflows.graphNodes;
    const entry = nodes.find((node) => node.id === "on_wake");
    const resume = nodes.find((node) => node.id === "resume_wake");
    const main = nodes.find((node) => node.id === "work");

    expect(entry?.data?.interruptRegion).toEqual({ source: "wake", handler: "on_wake" });
    expect(entry?.data?.interruptEntry).toBe(true);
    expect(resume?.data?.interruptEntry).toBe(false);
    expect(entry?.class).toContain("node-interrupt-region");
    expect(main?.data?.interruptRegion).toBeNull();
  });

  it("keeps the handler candidates free of main-flow nodes", () => {
    const workflows = useWorkflowsStore();
    workflows.selectWorkflow(workflow());
    workflows.scaffoldInterruptHandler("wake");

    const candidates = workflows.getHandlerCandidateNodeIds();

    expect(candidates).toContain("on_wake");
    expect(candidates).not.toContain("work");
    expect(candidates).not.toContain("start");
  });
});
