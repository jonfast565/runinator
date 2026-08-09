import { describe, expect, it } from "vitest";

import { buildGanttLayout } from "../run-gantt";
import { interruptDeclarations, interruptRegionOrigins } from "../interrupt-regions";
import type { WorkflowDefinition, WorkflowNodeRun, WorkflowRunDetail } from "../../domain/models";

/** start -> poll -> end, plus an isolated `refresh -> handled` handler region for `wake`. */
function snapshot(): WorkflowDefinition {
  return {
    name: "Interrupted",
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "poll" } } },
        { id: "poll", kind: "wait", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        {
          id: "refresh",
          kind: "action",
          transitions: { on_success: { $node: "handled" } },
        },
        { id: "handled", kind: "resume", parameters: { mode: "resume" } },
      ],
      metadata: { interrupts: [{ on: "wake", handler: "refresh" }] },
    },
  } as unknown as WorkflowDefinition;
}

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

function detail(nodes: WorkflowNodeRun[], withSnapshot = true): WorkflowRunDetail {
  return {
    run: {
      id: "run-1",
      workflow_id: "wf-1",
      status: "succeeded",
      created_at: "2026-07-16T00:00:00Z",
      started_at: "2026-07-16T00:00:00Z",
      finished_at: "2026-07-16T00:00:10Z",
      ...(withSnapshot ? { workflow_snapshot: snapshot() as never } : {}),
    },
    nodes,
  } as WorkflowRunDetail;
}

describe("interruptRegionOrigins", () => {
  it("reads the declared handlers", () => {
    expect(interruptDeclarations(snapshot())).toEqual([{ source: "wake", handler: "refresh" }]);
  });

  it("walks the whole region from its entry, not just the entry node", () => {
    const origins = interruptRegionOrigins(snapshot());

    expect(origins.get("refresh")).toEqual({ source: "wake", handler: "refresh" });
    expect(origins.get("handled")).toEqual({ source: "wake", handler: "refresh" });
  });

  it("leaves the main flow alone", () => {
    const origins = interruptRegionOrigins(snapshot());

    for (const id of ["start", "poll", "end"]) {
      expect(origins.has(id)).toBe(false);
    }
  });

  it("is empty for a workflow that declares no handlers", () => {
    expect(interruptRegionOrigins(null).size).toBe(0);
  });
});

describe("buildGanttLayout with an interrupt", () => {
  const nodes = [
    node({
      id: "n1",
      node_id: "poll",
      cursor_id: "cursor-main",
      created_at: "2026-07-16T00:00:00Z",
      started_at: "2026-07-16T00:00:00Z",
      finished_at: "2026-07-16T00:00:08Z",
    }),
    node({
      id: "n2",
      node_id: "refresh",
      // the handler's own ephemeral cursor, which no longer exists in run state.
      cursor_id: "cursor-handler",
      created_at: "2026-07-16T00:00:02Z",
      started_at: "2026-07-16T00:00:02Z",
      finished_at: "2026-07-16T00:00:03Z",
    }),
  ];

  it("attributes a handler's row to the interrupt that raised it", () => {
    const layout = buildGanttLayout(detail(nodes), Date.parse("2026-07-16T00:00:10Z"));
    const refresh = layout.rows.find((row) => row.nodeId === "refresh");

    expect(refresh?.interrupt).toEqual({ source: "wake", handler: "refresh" });
    expect(refresh?.cursorId).toBe("cursor-handler");
  });

  it("keeps main-flow rows unattributed", () => {
    const layout = buildGanttLayout(detail(nodes), Date.parse("2026-07-16T00:00:10Z"));
    const poll = layout.rows.find((row) => row.nodeId === "poll");

    expect(poll?.interrupt).toBeNull();
    expect(poll?.cursorId).toBe("cursor-main");
  });

  /** a handler is a side-channel, so it must not be reported as the run's critical path even when
   * it happens to be the longest bar. */
  it("never nominates a handler row as the bottleneck", () => {
    const slowHandler = [
      node({
        id: "n1",
        node_id: "poll",
        cursor_id: "cursor-main",
        created_at: "2026-07-16T00:00:00Z",
        started_at: "2026-07-16T00:00:00Z",
        finished_at: "2026-07-16T00:00:01Z",
      }),
      node({
        id: "n2",
        node_id: "refresh",
        cursor_id: "cursor-handler",
        created_at: "2026-07-16T00:00:01Z",
        started_at: "2026-07-16T00:00:01Z",
        finished_at: "2026-07-16T00:00:09Z",
      }),
    ];
    const layout = buildGanttLayout(detail(slowHandler), Date.parse("2026-07-16T00:00:10Z"));

    expect(layout.bottleneckNodeId).toBe("poll");
  });

  /** without the snapshot there is nothing to resolve regions against; rows must still build. */
  it("degrades to unattributed rows when the run carries no snapshot", () => {
    const layout = buildGanttLayout(detail(nodes, false), Date.parse("2026-07-16T00:00:10Z"));

    expect(layout.rows).toHaveLength(2);
    expect(layout.rows.every((row) => row.interrupt === null)).toBe(true);
  });
});
