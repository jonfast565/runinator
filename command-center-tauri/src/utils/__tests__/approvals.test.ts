import { describe, expect, it } from "vitest";
import type { WorkflowNodeRun } from "../../types/models";
import { approvalIdFromNodeRun, isApprovalWaitingStatus, selectWorkflowApprovalRecord } from "../approvals";

describe("approval utils", () => {
  it("treats workflow approval wait statuses consistently", () => {
    expect(isApprovalWaitingStatus("approval-required")).toBe(true);
    expect(isApprovalWaitingStatus("approval_required")).toBe(true);
    expect(isApprovalWaitingStatus("pending")).toBe(true);
    expect(isApprovalWaitingStatus("succeeded")).toBe(false);
  });

  it("reads approval ids from workflow node run state", () => {
    const nodeRun: WorkflowNodeRun = {
      id: 1,
      workflow_run_id: 10,
      node_id: "approval",
      task_run_id: null,
      status: "approval_required",
      attempt: 1,
      parameters: {},
      state: { approval_id: 42 },
      message: null
    };

    expect(approvalIdFromNodeRun(nodeRun)).toBe(42);
  });

  it("selects the pending approval for a workflow node", () => {
    const approval = selectWorkflowApprovalRecord(
      [
        { id: 2, workflow_run_id: 10, node_id: "approval", status: "approved" },
        { id: 3, workflow_run_id: 11, node_id: "approval", status: "pending" },
        { id: 4, workflow_run_id: 10, node_id: "approval", status: "pending" }
      ],
      10,
      "approval"
    );

    expect(approval?.id).toBe(4);
  });
});
