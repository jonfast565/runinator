import { describe, expect, it } from "vitest";
import type { WorkflowNodeRun } from "../../domain/models";
import {
  approvalIdFromNodeRun,
  isApprovalWaitingStatus,
  selectWorkflowApprovalRecord,
} from "../approvals";

describe("approval utils", () => {
  it("treats workflow approval wait statuses consistently", () => {
    expect(isApprovalWaitingStatus("approval-required")).toBe(true);
    expect(isApprovalWaitingStatus("approval_required")).toBe(true);
    expect(isApprovalWaitingStatus("pending")).toBe(true);
    expect(isApprovalWaitingStatus("succeeded")).toBe(false);
  });

  it("reads approval ids from workflow node run state", () => {
    const nodeRun: WorkflowNodeRun = {
      id: "00000000-0000-0000-0000-000000000001",
      workflow_run_id: "00000000-0000-0000-0000-000000000010",
      node_id: "approval",
      status: "approval_required",
      attempt: 1,
      parameters: {},
      state: { approval_id: "00000000-0000-0000-0000-000000000042" },
      message: null,
    };

    expect(approvalIdFromNodeRun(nodeRun)).toBe("00000000-0000-0000-0000-000000000042");
  });

  it("selects the pending approval for a workflow node", () => {
    const approval = selectWorkflowApprovalRecord(
      [
        {
          id: "00000000-0000-0000-0000-000000000002",
          workflow_run_id: "00000000-0000-0000-0000-000000000010",
          node_id: "approval",
          status: "approved",
        },
        {
          id: "00000000-0000-0000-0000-000000000003",
          workflow_run_id: "00000000-0000-0000-0000-000000000011",
          node_id: "approval",
          status: "pending",
        },
        {
          id: "00000000-0000-0000-0000-000000000004",
          workflow_run_id: "00000000-0000-0000-0000-000000000010",
          node_id: "approval",
          status: "pending",
        },
      ],
      "00000000-0000-0000-0000-000000000010",
      "approval",
    );

    expect(approval?.id).toBe("00000000-0000-0000-0000-000000000004");
  });
});
