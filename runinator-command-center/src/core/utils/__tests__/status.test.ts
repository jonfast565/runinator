import { describe, expect, it } from "vitest";
import {
  isTerminalWorkflowRunStatus,
  statusBadgeClass,
  statusBadgeLabel,
  statusClassForNode,
} from "../status";

describe("status utils", () => {
  it("maps terminal failures", () => {
    expect(statusBadgeClass("failed")).toBe("status-failed");
    expect(statusClassForNode("timed_out")).toBe("node-danger");
  });

  it("maps active statuses", () => {
    expect(statusBadgeClass("running")).toBe("status-running");
    expect(statusBadgeClass("queued")).toBe("status-waiting");
    expect(statusBadgeClass("debug_paused")).toBe("status-waiting");
    expect(statusClassForNode("waiting")).toBe("node-waiting");
    expect(statusClassForNode("approval_required")).toBe("node-waiting");
    expect(statusClassForNode("debug_paused")).toBe("node-warning");
  });

  it("labels a suspended run as parked", () => {
    expect(statusBadgeLabel("parked")).toBe("parked");
    expect(statusBadgeLabel("approval_required")).toBe("approval required");
    expect(statusBadgeLabel("running")).toBe("running");
  });

  it("identifies terminal workflow run statuses", () => {
    expect(isTerminalWorkflowRunStatus("succeeded")).toBe(true);
    expect(isTerminalWorkflowRunStatus("failed")).toBe(true);
    expect(isTerminalWorkflowRunStatus("timed_out")).toBe(true);
    expect(isTerminalWorkflowRunStatus("canceled")).toBe(true);
    expect(isTerminalWorkflowRunStatus("blocked")).toBe(false);
    expect(isTerminalWorkflowRunStatus("running")).toBe(false);
  });
});
