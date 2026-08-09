import { expect, it, vi } from "vitest";
import { useWorkflowsStore } from "../workflows";
import { compileWdl, decompileToWdl } from "../../../../core/api/commandCenterApi";
import { catalogMetadataService } from "../../../../core/services";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { testNodeKindCatalog } from "../../../../core/workflow/__tests__/catalog-fixtures";
import { WORKFLOW_ID, workflowDefinition, flushWorkflowSync, graphCentroid } from "./workflows-fixtures";

export function registerWorkflowSyncTests() {
  it("syncs json edits into the draft and wdl view", async () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "json sync"));
    workflows.workflowEditorMode = "json";
    vi.mocked(decompileToWdl).mockResolvedValue("workflow json_sync { start -> output }");

    workflows.workflowJson = JSON.stringify(
      {
        start: "start",
        nodes: [
          { id: "start", kind: "start", transitions: { next: { $node: "output-1" } } },
          {
            id: "output-1",
            kind: "output",
            parameters: { event_type: "workflow.output", data: { message: "hello" } },
            transitions: { next: { $node: "end" } },
          },
          { id: "end", kind: "end" },
          { id: "fail", kind: "fail" },
        ],
      },
      null,
      2,
    );

    expect(workflows.syncWorkflowJson()).toBe(true);
    await flushWorkflowSync();

    expect(
      (workflows.workflowDraft.definition as any).nodes.some((node: any) => node.id === "output-1"),
    ).toBe(true);
    expect(workflows.workflowWdl).toBe("workflow json_sync { start -> output }");
  });

  it("syncs wdl edits into the draft and json view", async () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "wdl sync"));
    workflows.workflowEditorMode = "wdl";
    vi.mocked(compileWdl).mockResolvedValue({
      id: WORKFLOW_ID,
      name: "wdl sync",
      version: "1.0.0",
      enabled: true,
      input_type: { type: "struct", fields: {} },
      definition: {
        start: "start",
        nodes: [
          { id: "start", kind: "start", transitions: { next: { $node: "output-1" } } },
          {
            id: "output-1",
            kind: "output",
            parameters: { event_type: "workflow.output", data: { message: "hello" } },
            transitions: { next: { $node: "end" } },
          },
          { id: "end", kind: "end" },
          { id: "fail", kind: "fail" },
        ],
      },
    });

    workflows.workflowWdl = "workflow wdl_sync { start -> output-1 }";

    expect(await workflows.syncWorkflowWdl()).toBe(true);

    expect(
      (workflows.workflowDraft.definition as any).nodes.some((node: any) => node.id === "output-1"),
    ).toBe(true);
    expect(JSON.parse(workflows.workflowJson)).toMatchObject({
      start: "start",
      nodes: expect.arrayContaining([expect.objectContaining({ id: "output-1" })]),
    });
  });

  it("duplicates nodes without carrying their outgoing connections", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "duplicate node"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");
    const centroid = graphCentroid(workflows.graphNodes);

    workflows.duplicateSelectedStep();

    const copy = workflows.ensureWorkflowNodes().find((node) => String(node.id).endsWith("_copy"));
    expect(copy).toMatchObject({
      kind: "action",
      action: { provider: "console", function: "run" },
      transitions: {},
    });
    expect(
      (workflows.workflowDraft.definition as any).ui?.layout?.nodes?.[copy!.id as string],
    ).toEqual(centroid);
    expect(workflows.graphEdges.some((edge) => edge.source === "task-1_copy")).toBe(false);
    expect(workflows.selectedStepId).toBe(copy?.id);
  });

  it("allows non-protected nodes to be locked", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "locked nodes"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "wait-1",
      kind: "wait",
      wait: { seconds: 5 },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("wait-1");

    workflows.stepEditor.locked = true;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1") as any)?.locked,
    ).toBe(true);
    expect(workflows.selectedStepKindLocked).toBe(true);
    expect(workflows.canRemoveSelectedStep).toBe(false);
  });

  it("marks and unmarks nodes as skipped", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "skipped nodes"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "wait-1",
      kind: "wait",
      wait: { seconds: 5 },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("wait-1");

    workflows.stepEditor.skipped = true;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1") as any)?.skipped,
    ).toBe(true);

    workflows.populateStepEditor("wait-1");
    workflows.stepEditor.skipped = false;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1") as any)?.skipped,
    ).toBeUndefined();
  });

  it("does not remove or change the kind of manually locked nodes", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "locked nodes"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      locked: true,
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");

    workflows.removeWorkflowNode("task-1");

    expect(workflows.ensureWorkflowNodes().some((node) => node.id === "task-1")).toBe(true);

    workflows.stepEditor.kind = "wait";

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("action node kind cannot be changed");
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "task-1") as any)?.kind,
    ).toBe("action");
  });

  it("updates workflowNodeKinds when catalog metadata loads", () => {
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
    catalogMetadataService.setState((state) => ({
      ...state,
      nodeKinds: [],
      loaded: false,
    }));

    const workflows = useWorkflowsStore();
    expect(workflows.workflowNodeKinds).toEqual([]);

    setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
    catalogMetadataService.setState((state) => ({
      ...state,
      nodeKinds: testNodeKindCatalog,
      loaded: true,
    }));

    expect(workflows.workflowNodeKinds).toEqual(
      testNodeKindCatalog.filter((entry) => entry.addable).map((entry) => entry.kind),
    );
  });}
