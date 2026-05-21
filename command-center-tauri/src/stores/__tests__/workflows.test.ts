import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useWorkflowsStore } from "../workflows";
import type { WorkflowDefinition, WorkflowRunDetail, WorkflowTrigger } from "../../types/models";

vi.mock("../../api/commandCenterApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/commandCenterApi")>()),
  fetchWorkflows: vi.fn(),
  fetchWorkflowRun: vi.fn(),
  patchWorkflowRunDebug: vi.fn(),
  saveWorkflowBundle: vi.fn()
}));

import { fetchWorkflowRun, fetchWorkflows, patchWorkflowRunDebug, saveWorkflowBundle } from "../../api/commandCenterApi";

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

  it("saves and reloads workflow triggers through the workflow bundle contract", async () => {
    const workflows = useWorkflowsStore();
    const draft = workflowDefinition(7, "bundle draft");
    Object.assign(workflows.workflowDraft, draft);
    workflows.workflowJson = JSON.stringify(draft.definition);
    workflows.workflowTriggers = [workflowTrigger(12, 7, "0 * * * *")];
    vi.mocked(saveWorkflowBundle).mockResolvedValue({
      workflows: [workflowDefinition(7, "bundle saved")],
      triggers: [workflowTrigger(12, 7, "30 * * * *")]
    });
    vi.mocked(fetchWorkflows).mockResolvedValue([workflowDefinition(7, "bundle saved")]);

    await workflows.saveSelectedWorkflow();

    expect(saveWorkflowBundle).toHaveBeenCalledWith({
      workflows: [expect.objectContaining({ id: 7, name: "bundle draft" })],
      triggers: [expect.objectContaining({ id: 12, workflow_id: 7, configuration: { cron: "0 * * * *", parameters: {} } })]
    });
    expect(workflows.workflowDraft.name).toBe("bundle saved");
    expect(workflows.workflowTriggers).toEqual([workflowTrigger(12, 7, "30 * * * *")]);
  });
});

function workflowDefinition(id: number, name: string): WorkflowDefinition {
  return {
    id,
    name,
    version: 1,
    enabled: true,
    input_schema: { type: "object" },
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
