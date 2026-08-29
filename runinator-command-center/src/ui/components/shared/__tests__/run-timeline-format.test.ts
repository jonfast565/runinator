import { describe, expect, it } from "vitest";

import type { WorkflowNodeRun } from "../../../../core/domain/models";
import { compareStepsAscending, stepTimestamp } from "../run-timeline-format";

function node(
  id: string,
  timestamps: Pick<WorkflowNodeRun, "created_at" | "started_at">,
): WorkflowNodeRun {
  return {
    id,
    workflow_run_id: "run-1",
    node_id: id,
    status: "succeeded",
    attempt: 1,
    parameters: {},
    message: null,
    ...timestamps,
  };
}

describe("run timeline formatting", () => {
  it("orders steps from the earliest execution time to the latest", () => {
    const queuedFirst = node("00000000-0000-7000-8000-000000000001", {
      created_at: "2026-08-29T03:00:00Z",
    });
    const startedEarlier = node("00000000-0000-7000-8000-000000000003", {
      created_at: "2026-08-29T03:10:00Z",
      started_at: "2026-08-29T03:01:00Z",
    });
    const startedLater = node("00000000-0000-7000-8000-000000000002", {
      created_at: "2026-08-29T03:02:00Z",
      started_at: "2026-08-29T03:02:00Z",
    });

    expect([startedLater, startedEarlier, queuedFirst].sort(compareStepsAscending)).toEqual([
      queuedFirst,
      startedEarlier,
      startedLater,
    ]);
  });

  it("uses the durable journal sequence when executions share the same second", () => {
    const greeting = node("greeting", {
      created_at: "2026-08-29T03:05:31Z",
    });
    greeting.state = { timeline_sequence: 1 };
    const end = node("end", {
      created_at: "2026-08-29T03:05:31Z",
    });
    end.state = { timeline_sequence: 3 };

    expect([end, greeting].sort(compareStepsAscending)).toEqual([greeting, end]);
  });

  it("formats valid timestamps locally and ignores malformed source values", () => {
    expect(stepTimestamp(node("valid", { created_at: "2026-08-29T03:05:31Z" }))).not.toBe("");
    expect(stepTimestamp(node("invalid", { created_at: "not-a-date" }))).toBe("");
  });
});
