import { expect, it, vi } from "vitest";
import { useWorkflowsStore } from "../workflows";
import { useProvidersStore } from "../providers";
import { decompileToWdl, fetchWorkflows, saveWorkflowWdl } from "../../../../core/api/commandCenterApi";
import { WORKFLOW_ID, TRIGGER_ID, workflowDefinition, graphCentroid, workflowTrigger, nestedWorkflowInputProvider, untypedActionProvider } from "./workflows-fixtures";

export function registerWorkflowAuthoringTests() {
  it("saves workflow edits as wdl and reloads workflow triggers", async () => {
    const workflows = useWorkflowsStore();
    const draft = workflowDefinition(WORKFLOW_ID, "bundle draft");
    draft.definition.ui = {
      layout: { nodes: { start: { x: 0, y: 0 }, end: { x: 270, y: 0 } } },
      edge_handles: { "start:next": { edgeStyle: "square", labelAnchor: { position: 0.25 } } },
    };
    Object.assign(workflows.workflowDraft, draft);
    workflows.workflowJson = JSON.stringify(draft.definition);
    workflows.workflowTriggers = [workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "0 * * * *")];
    vi.mocked(decompileToWdl).mockResolvedValue("workflow bundle_draft { start -> end }");
    vi.mocked(saveWorkflowWdl).mockResolvedValue({
      workflows: [workflowDefinition(WORKFLOW_ID, "bundle saved")],
      triggers: [workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "30 * * * *")],
    });
    vi.mocked(fetchWorkflows).mockResolvedValue([workflowDefinition(WORKFLOW_ID, "bundle saved")]);

    await workflows.saveSelectedWorkflow();

    expect(decompileToWdl).toHaveBeenCalledWith(
      expect.objectContaining({ id: WORKFLOW_ID, name: "bundle draft" }),
    );
    expect(saveWorkflowWdl).toHaveBeenCalledWith({
      source: "workflow bundle_draft { start -> end }",
      enabled: true,
      workflow_id: WORKFLOW_ID,
      ui: draft.definition.ui,
      triggers: [
        expect.objectContaining({
          id: TRIGGER_ID,
          workflow_id: WORKFLOW_ID,
          configuration: { cron: "0 * * * *", parameters: {} },
        }),
      ],
    });
    expect(workflows.workflowDraft.name).toBe("bundle saved");
    expect(workflows.workflowTriggers).toEqual([
      workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "30 * * * *"),
    ]);
  });

  it("validates nested typed workflow input shaped step parameters", async () => {
    const workflows = useWorkflowsStore();
    const providers = useProvidersStore();
    providers.providers = [nestedWorkflowInputProvider()];
    await workflows.selectWorkflow(workflowDefinition(WORKFLOW_ID, "nested input"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "prepare",
      kind: "action",
      action: {
        provider: "workflow-input",
        function: "prepare",
        timeout_seconds: 300,
        configuration: {},
      },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.openStepEditor("prepare");

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: "twice" },
        },
        strategy: { manual: true },
      },
    };

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe(
      "Workflow Input.environments.prod.retries must be an integer",
    );

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 },
        },
        strategy: { manual: true },
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "prepare") as any)?.action
        .configuration,
    ).toEqual({
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 },
        },
        strategy: { manual: true },
      },
    });
  });

  it("applies untyped action parameter objects and WDL expressions into action configuration", async () => {
    const workflows = useWorkflowsStore();
    const providers = useProvidersStore();
    providers.providers = [untypedActionProvider()];
    await workflows.selectWorkflow(workflowDefinition(WORKFLOW_ID, "untyped action"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "notify",
      kind: "action",
      action: { provider: "webhook", function: "send", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.openStepEditor("notify");

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      url: "https://example.test/hook",
      payload: {
        message: { $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] },
        urgent: true,
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "notify") as any)?.action
        .configuration,
    ).toEqual({
      url: "https://example.test/hook",
      payload: {
        message: { $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] },
        urgent: true,
      },
    });
  });

  it("exits inline node editing after a successful apply", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "inline edit"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");
    workflows.selectedGraphEdgeId = "edge-1";

    expect(workflows.submitInlineNodeEdit("task-1", "renamed", "Friendly Name")).toBe(true);

    // inline edits set the display name and never touch the configured action.
    const node = workflows.ensureWorkflowNodes().find((item) => item.id === "renamed");
    expect(node).toMatchObject({
      name: "Friendly Name",
      action: { provider: "console", function: "run" },
    });
    expect(workflows.selectedStepId).toBe("");
    expect(workflows.selectedGraphEdgeId).toBe("");
  });

  it("keeps inline node editing open when apply fails", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "inline edit"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");

    expect(workflows.submitInlineNodeEdit("task-1", "end", "console.echo")).toBe(false);

    expect(workflows.selectedStepId).toBe("task-1");
  });

  it("does not remove protected terminal and entry nodes", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "protected nodes"));

    workflows.populateStepEditor("start");

    expect(workflows.selectedStepKindLocked).toBe(true);
    expect(workflows.canRemoveSelectedStep).toBe(false);

    workflows.removeWorkflowNode("start");
    workflows.removeWorkflowNode("end");
    workflows.removeWorkflowNode("fail");

    expect(workflows.ensureWorkflowNodes().map((node) => node.id)).toEqual([
      "start",
      "end",
      "fail",
    ]);
  });

  it("does not allow protected node kinds to be changed", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "protected nodes"));
    workflows.populateStepEditor("start");

    workflows.stepEditor.kind = "action";

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("start node kind cannot be changed");
    expect((workflows.ensureWorkflowNodes().find((node) => node.id === "start") as any)?.kind).toBe(
      "start",
    );
  });

  it("creates new graph nodes without wiring them immediately", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "standalone node"));
    workflows.workflowJson = JSON.stringify(workflows.workflowDraft.definition);
    const centroid = graphCentroid(workflows.graphNodes);

    workflows.addWorkflowNode("approval");

    const created = workflows
      .ensureWorkflowNodes()
      .find((node) => node.kind === "approval" && String(node.id).startsWith("approval"));
    expect(created).toMatchObject({
      kind: "approval",
      parameters: { approval_type: "generic", prompt: "Approval required" },
      transitions: {},
    });
    expect(
      (workflows.workflowDraft.definition as any).ui?.layout?.nodes?.[created!.id as string],
    ).toEqual(centroid);
    expect(workflows.graphEdges.some((edge) => edge.target === created?.id)).toBe(false);
    expect(workflows.selectedStepId).toBe(created?.id);
  });

  it("treats the connected-node action as a standalone node creation", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "standalone node"));
    workflows.workflowJson = JSON.stringify(workflows.workflowDraft.definition);
    workflows.selectedStepId = "start";
    const centroid = graphCentroid(workflows.graphNodes);

    workflows.addConnectedWorkflowNode("output");

    const created = workflows
      .ensureWorkflowNodes()
      .find((node) => node.kind === "output" && String(node.id).startsWith("output"));
    expect(created).toMatchObject({
      kind: "output",
      parameters: { event_type: "workflow.output", data: {} },
      transitions: {},
    });
    expect(
      (workflows.workflowDraft.definition as any).ui?.layout?.nodes?.[created!.id as string],
    ).toEqual(centroid);
    expect(workflows.graphEdges.some((edge) => edge.target === created?.id)).toBe(false);
    expect(workflows.selectedStepId).toBe(created?.id);
  });

  it("keeps output payloads as validated raw json", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "output payload"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "output-1",
      kind: "output",
      parameters: { event_type: "workflow.output", data: null },
      transitions: {},
    });

    workflows.populateStepEditor("output-1");

    // set output data directly in nodeDraft.parameters (the catalog field editor writes here).
    (workflows.stepEditor.nodeDraft as any).parameters = {
      event_type: "workflow.output",
      data: { message: "hello", retries: [1, 2], nested: { ok: true } },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "output-1") as any)?.parameters,
    ).toEqual({
      event_type: "workflow.output",
      data: { message: "hello", retries: [1, 2], nested: { ok: true } },
    });
  });

  it("keeps WDL-lowered output payload expressions valid", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "output expression"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "output-1",
      kind: "output",
      parameters: { event_type: "workflow.output", data: {} },
      transitions: {},
    });

    workflows.populateStepEditor("output-1");
    // write expression value directly into nodeDraft (as the catalog field editor would).
    (workflows.stepEditor.nodeDraft as any).parameters.data = { $ref: { params: ["message"] } };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.stepEditorError).toBe("");
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "output-1") as any)?.parameters
        ?.data,
    ).toEqual({ $ref: { params: ["message"] } });
  });

  it("applies config node WDL fields without validation errors", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "config editor"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "config-1",
      kind: "config",
      parameters: {
        name: "release",
        metadata: { owner: "platform" },
      },
      transitions: {},
    });

    workflows.populateStepEditor("config-1");
    // write config fields directly into nodeDraft.parameters (as the catalog field editor would).
    (workflows.stepEditor.nodeDraft as any).parameters = {
      name: { $ref: { params: ["release_name"] } },
      metadata: {
        source: { $ref: { prev: ["artifact"] } },
        approved: true,
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.stepEditorError).toBe("");
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "config-1") as any)?.parameters,
    ).toEqual({
      name: { $ref: { params: ["release_name"] } },
      metadata: {
        source: { $ref: { prev: ["artifact"] } },
        approved: true,
      },
    });
  });

}
