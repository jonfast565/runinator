import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import { useProvidersStore } from "../providers";
import type { ProviderMetadata, WorkflowDefinition, WorkflowRunDetail, WorkflowTrigger } from "../../types/models";

vi.mock("../../api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/commandCenterApi")>()),
  fetchWorkflows: vi.fn(),
  fetchWorkflowRun: vi.fn(),
  patchWorkflowRunDebug: vi.fn(),
  saveWorkflowWdl: vi.fn(),
  decompileToWdl: vi.fn()
}));

import { decompileToWdl, fetchWorkflowRun, fetchWorkflows, patchWorkflowRunDebug, saveWorkflowWdl } from "../../api/commandCenterApi";

describe("workflow run detail state", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal("window", {
      clearTimeout: () => undefined,
      setTimeout: () => 0
    });
    vi.clearAllMocks();
  });

  it("does not let older HTTP fetches overwrite a WebSocket push", async () => {
    const workflows = useWorkflowsStore();
    let resolveFetch: (detail: WorkflowRunDetail) => void = () => undefined;
    vi.mocked(fetchWorkflowRun).mockReturnValue(new Promise((resolve) => {
      resolveFetch = resolve;
    }));

    const request = workflows.fetchWorkflowRunDetail(7, true);
    const pushed = workflowDetail(7, "running", "ws");
    workflows.setWorkflowRunDetail(pushed);

    resolveFetch(workflowDetail(7, "queued", "http"));
    await request;

    expect(workflows.workflowRunDetail?.run.status).toBe("running");
    expect(workflows.workflowRunDetail?.run.message).toBe("ws");
  });

  it("keeps an optimistic breakpoint through stale pushes until the server confirms it", async () => {
    const workflows = useWorkflowsStore();
    workflows.setWorkflowRunDetail(workflowDetail(7, "debug_paused", "initial", []));
    vi.mocked(patchWorkflowRunDebug).mockResolvedValue({ success: true, message: "ok" });
    vi.mocked(fetchWorkflowRun).mockResolvedValue(workflowDetail(7, "debug_paused", "stale http", []));

    await workflows.toggleBreakpoint("task-1");

    expect(workflows.currentBreakpoints).toEqual(["task-1"]);

    workflows.setWorkflowRunDetail(workflowDetail(7, "running", "stale ws", []));

    expect(workflows.workflowRunDetail?.run.status).toBe("running");
    expect(workflows.workflowRunDetail?.run.message).toBe("stale ws");
    expect(workflows.currentBreakpoints).toEqual(["task-1"]);

    workflows.setWorkflowRunDetail(workflowDetail(7, "running", "confirmed ws", ["task-1"]));
    workflows.setWorkflowRunDetail(workflowDetail(7, "running", "next ws", []));

    expect(workflows.workflowRunDetail?.run.message).toBe("next ws");
    expect(workflows.currentBreakpoints).toEqual([]);
  });

  it("saves workflow edits as wdl and reloads workflow triggers", async () => {
    const workflows = useWorkflowsStore();
    const draft = workflowDefinition(7, "bundle draft");
    Object.assign(workflows.workflowDraft, draft);
    workflows.workflowJson = JSON.stringify(draft.definition);
    workflows.workflowTriggers = [workflowTrigger(12, 7, "0 * * * *")];
    vi.mocked(decompileToWdl).mockResolvedValue("workflow bundle_draft { start -> end }");
    vi.mocked(saveWorkflowWdl).mockResolvedValue({
      workflows: [workflowDefinition(7, "bundle saved")],
      triggers: [workflowTrigger(12, 7, "30 * * * *")]
    });
    vi.mocked(fetchWorkflows).mockResolvedValue([workflowDefinition(7, "bundle saved")]);

    await workflows.saveSelectedWorkflow();

    expect(decompileToWdl).toHaveBeenCalledWith(expect.objectContaining({ id: 7, name: "bundle draft" }));
    expect(saveWorkflowWdl).toHaveBeenCalledWith({
      source: "workflow bundle_draft { start -> end }",
      enabled: true,
      workflow_id: 7,
      triggers: [expect.objectContaining({ id: 12, workflow_id: 7, configuration: { cron: "0 * * * *", parameters: {} } })]
    });
    expect(workflows.workflowDraft.name).toBe("bundle saved");
    expect(workflows.workflowTriggers).toEqual([workflowTrigger(12, 7, "30 * * * *")]);
  });

  it("validates nested typed workflow input shaped step parameters", async () => {
    const workflows = useWorkflowsStore();
    const providers = useProvidersStore();
    providers.providers = [nestedWorkflowInputProvider()];
    await workflows.selectWorkflow(workflowDefinition(7, "nested input"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "prepare",
      kind: "action",
      action: { provider: "workflow-input", function: "prepare", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.openStepEditor("prepare");

    workflows.stepEditor.parameters_json = JSON.stringify({
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: "twice" }
        },
        strategy: { manual: true }
      }
    });

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("Workflow Input.environments.prod.retries must be an integer");

    workflows.stepEditor.parameters_json = JSON.stringify({
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 }
        },
        strategy: { manual: true }
      }
    });

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "prepare")?.action.configuration).toEqual({
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 }
        },
        strategy: { manual: true }
      }
    });
  });

  it("exits inline node editing after a successful apply", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "inline edit"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.populateStepEditor("task-1");
    workflows.selectedGraphEdgeId = "edge-1";

    expect(workflows.submitInlineNodeEdit("task-1", "renamed", "Friendly Name")).toBe(true);

    // inline edits set the display name and never touch the configured action.
    const node = workflows.ensureWorkflowNodes().find((item) => item.id === "renamed");
    expect(node).toMatchObject({ name: "Friendly Name", action: { provider: "console", function: "run" } });
    expect(workflows.selectedStepId).toBe("");
    expect(workflows.selectedGraphEdgeId).toBe("");
  });

  it("keeps inline node editing open when apply fails", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "inline edit"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.populateStepEditor("task-1");

    expect(workflows.submitInlineNodeEdit("task-1", "end", "console.echo")).toBe(false);

    expect(workflows.selectedStepId).toBe("task-1");
  });

  it("does not remove protected terminal and entry nodes", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "protected nodes"));

    workflows.populateStepEditor("start");

    expect(workflows.selectedStepKindLocked).toBe(true);
    expect(workflows.canRemoveSelectedStep).toBe(false);

    workflows.removeWorkflowNode("start");
    workflows.removeWorkflowNode("end");
    workflows.removeWorkflowNode("fail");

    expect(workflows.ensureWorkflowNodes().map((node) => node.id)).toEqual(["start", "end", "fail"]);
  });

  it("does not allow protected node kinds to be changed", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "protected nodes"));
    workflows.populateStepEditor("start");

    workflows.stepEditor.kind = "action";

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("start node kind cannot be changed");
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "start")?.kind).toBe("start");
  });

  it("allows non-protected nodes to be locked", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "locked nodes"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "wait-1",
      kind: "wait",
      wait: { seconds: 5 },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.populateStepEditor("wait-1");

    workflows.stepEditor.locked = true;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1")?.locked).toBe(true);
    expect(workflows.selectedStepKindLocked).toBe(true);
    expect(workflows.canRemoveSelectedStep).toBe(false);
  });

  it("marks and unmarks nodes as skipped", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "skipped nodes"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "wait-1",
      kind: "wait",
      wait: { seconds: 5 },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.populateStepEditor("wait-1");

    workflows.stepEditor.skipped = true;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1")?.skipped).toBe(true);

    workflows.populateStepEditor("wait-1");
    workflows.stepEditor.skipped = false;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "wait-1")?.skipped).toBeUndefined();
  });

  it("does not remove or change the kind of manually locked nodes", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(7, "locked nodes"));
    workflows.workflowDraft.definition.nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      locked: true,
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { "$node": "end" } }
    });
    workflows.populateStepEditor("task-1");

    workflows.removeWorkflowNode("task-1");

    expect(workflows.ensureWorkflowNodes().some((node) => node.id === "task-1")).toBe(true);

    workflows.stepEditor.kind = "wait";

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("action node kind cannot be changed");
    expect(workflows.ensureWorkflowNodes().find((node) => node.id === "task-1")?.kind).toBe("action");
  });
});

function workflowDefinition(id: number, name: string): WorkflowDefinition {
  return {
    id,
    name,
    version: 1,
    enabled: true,
    input_type: { type: "struct", fields: {} },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { "$node": "end" } } },
        { id: "end", kind: "end" },
        { id: "fail", kind: "fail" }
      ]
    }
  };
}

function workflowTrigger(id: number, workflowId: number, cron: string): WorkflowTrigger {
  return {
    id,
    workflow_id: workflowId,
    kind: "cron",
    enabled: true,
    configuration: { cron, parameters: {} },
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {}
  };
}

function workflowDetail(id: number, status: string, message: string, breakpoints: string[] = []): WorkflowRunDetail {
  return {
    run: {
      id,
      workflow_id: 1,
      status,
      parameters: {},
      state: { debug: { enabled: true, breakpoints } },
      active_node_id: null,
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: null,
      message
    },
    nodes: []
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
                        retries: { required: false, ty: { type: "integer" } }
                      }
                    }
                  }
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
                          manual: { required: true, ty: { type: "boolean" } }
                        }
                      }
                    ]
                  }
                }
              }
            }
          }
        ]
      }
    ]
  };
}
