import { describe, expect, it } from "vitest";

import type { WorkflowNodeRun } from "../../domain/models";
import { timelineEventCategory } from "../timeline-events";

function node(partial: Partial<WorkflowNodeRun> = {}): WorkflowNodeRun {
  return {
    id: "event-1",
    workflow_run_id: "run-1",
    node_id: "publish",
    status: "succeeded",
    attempt: 1,
    parameters: {},
    message: null,
    ...partial,
  };
}

describe("timelineEventCategory", () => {
  it("labels authored workflow events as user events", () => {
    expect(timelineEventCategory(node())).toEqual({
      id: "user",
      label: "User",
      title: "An authored workflow lifecycle event.",
    });
  });

  it("recognizes legacy workspace phases as system events", () => {
    expect(
      timelineEventCategory(node({ state: { workspace_phase: "workspace.restore.materialize" } }))
        .id,
    ).toBe("system");
  });

  it("preserves unknown explicit categories for future filters", () => {
    expect(timelineEventCategory(node({ timeline_category: "orchestration" }))).toEqual({
      id: "orchestration",
      label: "Orchestration",
      title: "Workflow lifecycle event categorized as Orchestration.",
    });
  });
});
