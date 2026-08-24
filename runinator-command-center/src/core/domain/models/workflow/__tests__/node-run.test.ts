import { describe, expect, it } from "vitest";

import { workflowEffectId, type WorkflowNodeRun } from "../node-run";

function node(state?: WorkflowNodeRun["state"]): WorkflowNodeRun {
  return {
    id: "timeline-row-id",
    workflow_run_id: "run-id",
    node_id: "step",
    status: "failed",
    attempt: 0,
    parameters: {},
    state,
    message: "boom",
  };
}

describe("workflowEffectId", () => {
  it("returns the durable effect id for an effect-backed timeline row", () => {
    expect(workflowEffectId(node({ effect_id: "effect-id" }))).toBe("effect-id");
  });

  it("does not confuse a journal-only row id with an effect id", () => {
    expect(workflowEffectId(node({ journal_entry_id: "timeline-row-id" }))).toBeNull();
  });
});
