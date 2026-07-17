import { describe, expect, it } from "vitest";

import { buildGanttLayout, formatDuration } from "../run-gantt";
import type { WorkflowNodeRun, WorkflowRunDetail } from "../../domain/models";

function node(partial: Partial<WorkflowNodeRun> & { id: string; node_id: string }): WorkflowNodeRun {
  return {
    workflow_run_id: "run-1",
    status: "succeeded",
    attempt: 1,
    parameters: {},
    message: null,
    ...partial,
  };
}

function detail(nodes: WorkflowNodeRun[], run?: Partial<WorkflowRunDetail["run"]>): WorkflowRunDetail {
  return {
    run: {
      id: "run-1",
      workflow_id: "wf-1",
      status: "succeeded",
      created_at: "2026-07-16T00:00:00Z",
      started_at: "2026-07-16T00:00:00Z",
      finished_at: "2026-07-16T00:00:10Z",
      ...run,
    },
    nodes,
  };
}

describe("buildGanttLayout", () => {
  it("returns an empty layout when there are no nodes", () => {
    const layout = buildGanttLayout(detail([]), Date.now());
    expect(layout.rows).toEqual([]);
    expect(layout.bottleneckId).toBeNull();
  });

  it("positions bars proportionally across the run span", () => {
    const layout = buildGanttLayout(
      detail([
        node({
          id: "a",
          node_id: "first",
          created_at: "2026-07-16T00:00:00Z",
          started_at: "2026-07-16T00:00:00Z",
          finished_at: "2026-07-16T00:00:02Z",
        }),
        node({
          id: "b",
          node_id: "second",
          created_at: "2026-07-16T00:00:02Z",
          started_at: "2026-07-16T00:00:05Z",
          finished_at: "2026-07-16T00:00:10Z",
        }),
      ]),
      Date.parse("2026-07-16T00:00:10Z"),
    );

    expect(layout.totalMs).toBe(10_000);
    const [first, second] = layout.rows;
    expect(first.barLeftPct).toBeCloseTo(0, 5);
    expect(first.barWidthPct).toBeCloseTo(20, 5);
    // the second node parks from 00:02 → 00:05 before running to 00:10.
    expect(second.waitLeftPct).toBeCloseTo(20, 5);
    expect(second.waitWidthPct).toBeCloseTo(30, 5);
    expect(second.barLeftPct).toBeCloseTo(50, 5);
    expect(second.barWidthPct).toBeCloseTo(50, 5);
    expect(second.waitMs).toBe(3000);
  });

  it("flags the longest active segment as the critical-path bottleneck", () => {
    const layout = buildGanttLayout(
      detail([
        node({
          id: "a",
          node_id: "quick",
          started_at: "2026-07-16T00:00:00Z",
          finished_at: "2026-07-16T00:00:01Z",
        }),
        node({
          id: "b",
          node_id: "slow",
          started_at: "2026-07-16T00:00:01Z",
          finished_at: "2026-07-16T00:00:09Z",
        }),
      ]),
      Date.parse("2026-07-16T00:00:10Z"),
    );

    expect(layout.bottleneckId).toBe("b");
    expect(layout.bottleneckNodeId).toBe("slow");
    expect(layout.rows.find((row) => row.id === "b")?.critical).toBe(true);
    expect(layout.rows.find((row) => row.id === "a")?.critical).toBe(false);
  });

  it("counts an in-flight node up to now", () => {
    const now = Date.parse("2026-07-16T00:00:06Z");
    const layout = buildGanttLayout(
      detail(
        [
          node({
            id: "a",
            node_id: "running",
            status: "running",
            started_at: "2026-07-16T00:00:00Z",
            finished_at: null,
          }),
        ],
        { status: "running", finished_at: null },
      ),
      now,
    );

    const row = layout.rows[0];
    expect(row.running).toBe(true);
    expect(row.durationMs).toBe(6000);
  });
});

describe("formatDuration", () => {
  it("formats milliseconds, seconds, and minutes", () => {
    expect(formatDuration(250)).toBe("250ms");
    expect(formatDuration(2500)).toBe("2.5s");
    expect(formatDuration(90_000)).toBe("1m 30s");
  });
});
