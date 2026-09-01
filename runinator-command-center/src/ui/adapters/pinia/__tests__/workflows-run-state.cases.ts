import { expect, it, vi } from "vitest";
import { useWorkflowsStore } from "../workflows";
import type { WorkflowRunDetail } from "../../../../core/domain/models";
import {
  closeGate,
  fetchGates,
  fetchWorkflowRun,
  openGate,
} from "../../../../core/api/commandCenterApi";
import {
  RUN_ID,
  WORKFLOW_ID,
  flushWorkflowSync,
  workflowDefinition,
  workflowDetail,
  waitingGateWorkflowDetail,
} from "./workflows-fixtures";

export function registerWorkflowRunStateTests() {
  it("opens the debug run input dialog immediately", async () => {
    const workflows = useWorkflowsStore();
    const workflow = workflowDefinition(WORKFLOW_ID, "typed input");
    workflow.input_type = {
      type: "struct",
      fields: {
        environment: { required: true, ty: { type: "string" } },
      },
    };
    workflows.workflows = [workflow];
    await workflows.selectWorkflow(workflow);

    // Prime the computed binding as the mounted view does before the click.
    expect(workflows.runInputOpen).toBe(false);

    await workflows.runSelectedWorkflowDebug();

    expect(workflows.runInputOpen).toBe(true);
    expect(workflows.runInputDebug).toBe(true);
    expect(workflows.runInputDraft).toEqual({ environment: "" });
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

  it("retains projected VM node history when the run stream sends its lightweight envelope", () => {
    const workflows = useWorkflowsStore();
    const projected = waitingGateWorkflowDetail();
    workflows.setWorkflowRunDetail(projected);

    workflows.setWorkflowRunDetail({
      run: {
        ...projected.run,
        status: "succeeded",
        workflow_snapshot: undefined,
      },
      nodes: [],
      execution_state: {},
    });

    expect(workflows.workflowRunDetail).toMatchObject({
      run: {
        status: "succeeded",
        workflow_snapshot: projected.run.workflow_snapshot,
      },
      nodes: projected.nodes,
    });
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
}
