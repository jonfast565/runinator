import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import { useProvidersStore } from "../providers";
import type {
  ProviderMetadata,
  WorkflowDefinition,
  WorkflowRunDetail,
  WorkflowTrigger,
} from "../../../../core/domain/models";

vi.mock("../../../../core/api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/api/commandCenterApi")>()),
  closeGate: vi.fn(),
  compileWdl: vi.fn(),
  fetchGates: vi.fn(),
  fetchWorkflows: vi.fn(),
  fetchWorkflowRun: vi.fn(),
  openGate: vi.fn(),
  patchWorkflowRunDebug: vi.fn(),
  saveWorkflowWdl: vi.fn(),
  decompileToWdl: vi.fn(),
}));

import {
  closeGate,
  compileWdl,
  decompileToWdl,
  fetchGates,
  fetchWorkflowRun,
  fetchWorkflows,
  openGate,
  patchWorkflowRunDebug,
  saveWorkflowWdl,
} from "../../../../core/api/commandCenterApi";
import { setWorkflowCatalogs } from "../../../../core/workflow/catalog-registry";
import { testNodeKindCatalog } from "../../../../core/workflow/__tests__/catalog-fixtures";

const WORKFLOW_ID = "00000000-0000-0000-0000-000000000007";
const RUN_ID = "00000000-0000-0000-0000-000000000070";
const NODE_RUN_ID = "00000000-0000-0000-0000-000000000071";
const TRIGGER_ID = "00000000-0000-0000-0000-000000000012";

describe("workflow run detail state", () => {
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
    vi.mocked(fetchGates).mockResolvedValue([]);
    vi.mocked(decompileToWdl).mockResolvedValue("workflow stub { start -> end }");
  });

  afterEach(() => {
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
  });

  it("does not let older HTTP fetches overwrite a WebSocket push", async () => {
    const workflows = useWorkflowsStore();
    let resolveFetch: (detail: WorkflowRunDetail) => void = () => undefined;
    vi.mocked(fetchWorkflowRun).mockReturnValue(
      new Promise((resolve) => {
        resolveFetch = resolve;
      }),
    );

    const request = workflows.fetchWorkflowRunDetail(RUN_ID, true);
    const pushed = workflowDetail(RUN_ID, "running", "ws");
    workflows.setWorkflowRunDetail(pushed);

    resolveFetch(workflowDetail(RUN_ID, "queued", "http"));
    await request;

    expect(workflows.workflowRunDetail?.run.status).toBe("running");
    expect(workflows.workflowRunDetail?.run.message).toBe("ws");
  });

  it("keeps an optimistic breakpoint through stale pushes until the server confirms it", async () => {
    const workflows = useWorkflowsStore();
    workflows.setWorkflowRunDetail(workflowDetail(RUN_ID, "debug_paused", "initial", []));
    vi.mocked(patchWorkflowRunDebug).mockResolvedValue({ success: true, message: "ok" });
    vi.mocked(fetchWorkflowRun).mockResolvedValue(
      workflowDetail(RUN_ID, "debug_paused", "stale http", []),
    );

    await workflows.toggleBreakpoint("task-1");

    expect(workflows.currentBreakpoints).toEqual(["task-1"]);

    workflows.setWorkflowRunDetail(workflowDetail(RUN_ID, "running", "stale ws", []));

    expect(workflows.workflowRunDetail?.run.status).toBe("running");
    expect(workflows.workflowRunDetail?.run.message).toBe("stale ws");
    expect(workflows.currentBreakpoints).toEqual(["task-1"]);

    workflows.setWorkflowRunDetail(workflowDetail(RUN_ID, "running", "confirmed ws", ["task-1"]));
    workflows.setWorkflowRunDetail(workflowDetail(RUN_ID, "running", "next ws", []));

    expect(workflows.workflowRunDetail?.run.message).toBe("next ws");
    expect(workflows.currentBreakpoints).toEqual([]);
  });

  it("loads run gates for waiting gate nodes and refreshes after resolving them", async () => {
    const workflows = useWorkflowsStore();
    vi.mocked(fetchGates)
      .mockResolvedValueOnce([
        {
          id: "gate-1",
          workflow_run_id: RUN_ID,
          node_id: "gate-1",
          kind: "manual",
          status: "pending",
          label: "Deploy window",
        },
      ])
      .mockResolvedValueOnce([
        {
          id: "gate-1",
          workflow_run_id: RUN_ID,
          node_id: "gate-1",
          kind: "manual",
          status: "open",
          label: "Deploy window",
          reason: "Window approved",
        },
      ]);
    vi.mocked(openGate).mockResolvedValue({ success: true, message: "Gate opened" });
    vi.mocked(closeGate).mockResolvedValue({ success: true, message: "Gate closed" });
    vi.mocked(fetchWorkflowRun).mockResolvedValue(waitingGateWorkflowDetail());

    workflows.setWorkflowRunDetail(waitingGateWorkflowDetail());
    await flushWorkflowSync();

    expect(fetchGates).toHaveBeenCalledWith(RUN_ID);
    expect(workflows.workflowRunGates).toHaveLength(1);
    expect(workflows.runGraphNodes.find((node) => node.id === "gate-1")?.data).toMatchObject({
      gate: expect.objectContaining({ id: "gate-1", status: "pending" }),
      allowGateResolution: true,
      readOnly: true,
    });

    await workflows.resolveWorkflowRunGate("gate-1", "open", "Window approved");

    expect(openGate).toHaveBeenCalledWith("gate-1", "Window approved");
    expect(closeGate).not.toHaveBeenCalled();
    expect(fetchWorkflowRun).toHaveBeenCalledWith(RUN_ID);
    expect(workflows.workflowRunGates[0]).toMatchObject({
      id: "gate-1",
      status: "open",
      reason: "Window approved",
    });
  });

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
});

function workflowDefinition(id: string, name: string): WorkflowDefinition {
  return {
    id,
    name,
    version: "1.0.0",
    enabled: true,
    input_type: { type: "struct", fields: {} },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "fail", kind: "fail" },
      ],
    },
  };
}

async function flushWorkflowSync() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function graphCentroid(nodes: { position?: { x: number; y: number } }[]): { x: number; y: number } {
  const positioned = nodes
    .map((node) => ({ x: Number(node.position?.x), y: Number(node.position?.y) }))
    .filter((position) => Number.isFinite(position.x) && Number.isFinite(position.y));
  const totals = positioned.reduce(
    (sum, position) => ({ x: sum.x + position.x, y: sum.y + position.y }),
    { x: 0, y: 0 },
  );
  return {
    x: Math.round(totals.x / positioned.length),
    y: Math.round(totals.y / positioned.length),
  };
}

function workflowTrigger(id: string, workflowId: string, cron: string): WorkflowTrigger {
  return {
    id,
    workflow_id: workflowId,
    kind: "cron",
    enabled: true,
    configuration: { cron, parameters: {} },
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {},
  };
}

function workflowDetail(
  id: string,
  status: string,
  message: string,
  breakpoints: string[] = [],
): WorkflowRunDetail {
  return {
    run: {
      id,
      workflow_id: WORKFLOW_ID,
      status,
      parameters: {},
      state: { debug: { enabled: true, breakpoints } },
      active_node_id: null,
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: null,
      message,
    },
    nodes: [],
  };
}

function waitingGateWorkflowDetail(): WorkflowRunDetail {
  return {
    run: {
      id: RUN_ID,
      workflow_id: WORKFLOW_ID,
      status: "waiting",
      parameters: {},
      state: {},
      active_node_id: "gate-1",
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: null,
      message: "waiting on gate",
      workflow_snapshot: {
        id: WORKFLOW_ID,
        name: "gate flow",
        version: "1.0.0",
        enabled: true,
        input_type: { type: "struct", fields: {} },
        definition: {
          start: "start",
          nodes: [
            { id: "start", kind: "start", transitions: { next: { $node: "gate-1" } } },
            {
              id: "gate-1",
              kind: "gate",
              parameters: { kind: "manual", label: "Deploy window" },
              transitions: { next: { $node: "end" } },
            },
            { id: "end", kind: "end" },
            { id: "fail", kind: "fail" },
          ],
        },
      },
    },
    nodes: [
      {
        id: NODE_RUN_ID,
        workflow_run_id: RUN_ID,
        node_id: "gate-1",
        status: "waiting",
        attempt: 1,
        parameters: {},
        state: { gate_id: "gate-1", poll_interval: 30 },
        message: "waiting",
      },
    ],
  };
}

function nestedWorkflowInputProvider(): ProviderMetadata {
  return {
    name: "workflow-input",
    metadata: { credential_scopes: [], contract: null },
    actions: [
      {
        function_name: "prepare",
        description: null,
        results: [],
        parameters: [
          {
            name: "workflow_input",
            label: "Workflow Input",
            description: null,
            required: true,
            secret: false,
            ty: {
              type: "struct",
              fields: {
                target: { required: true, ty: { type: "string" } },
                environments: {
                  required: true,
                  ty: {
                    type: "map",
                    values: {
                      type: "struct",
                      fields: {
                        url: { required: true, ty: { type: "string" } },
                        retries: { required: false, ty: { type: "integer" } },
                      },
                    },
                  },
                },
                strategy: {
                  required: true,
                  ty: {
                    type: "union",
                    variants: [
                      { type: "string" },
                      {
                        type: "struct",
                        fields: {
                          manual: { required: true, ty: { type: "boolean" } },
                        },
                      },
                    ],
                  },
                },
              },
            },
          },
        ],
      },
    ],
  };
}

function untypedActionProvider(): ProviderMetadata {
  return {
    name: "webhook",
    metadata: { credential_scopes: [], contract: null },
    actions: [
      {
        function_name: "send",
        description: null,
        results: [],
        parameters: [],
      },
    ],
  };
}
