import type { SimulationRun } from "../domain/models";

export type SimTone = "ok" | "warn" | "bad" | "muted";

// one display row for a walked node in a dry-run preview.
export interface SimPreviewRow {
  nodeId: string;
  kind: string;
  status: string;
  tone: SimTone;
  // the node the walk routed to next, when it had an outgoing edge.
  branch: string | null;
  // a short transition reason mirroring the reducer.
  note: string | null;
}

// a display-ready view of a `SimulationRun`.
export interface SimPreview {
  rows: SimPreviewRow[];
  status: string;
  tone: SimTone;
  // count of distinct nodes visited.
  reachedCount: number;
  // set when the walk could not continue.
  error: string | null;
  // pretty-printed final output, or null when the run produced no output.
  outputJson: string | null;
}

const OK = new Set(["succeeded", "completed"]);
const WARN = new Set(["waiting", "running", "queued", "blocked", "skipped"]);
const BAD = new Set(["failed", "timed_out", "canceled", "cancelled", "stuck", "error"]);

// map a workflow/node status to a color tone, defaulting to muted for anything unrecognised.
export function simTone(status: string): SimTone {
  const normalized = status.toLowerCase();

  if (OK.has(normalized)) {
    return "ok";
  }

  if (BAD.has(normalized)) {
    return "bad";
  }

  if (WARN.has(normalized)) {
    return "warn";
  }

  return "muted";
}

// build a display-ready preview from a dry-run `SimulationRun`. Pure: no I/O, no store access.
export function buildSimPreview(run: SimulationRun): SimPreview {
  const rows: SimPreviewRow[] = run.steps.map((step) => ({
    nodeId: step.node_id,
    kind: step.kind,
    status: step.status,
    tone: simTone(step.status),
    branch: step.next ?? null,
    note: step.note ?? null,
  }));

  const reached = new Set(rows.map((row) => row.nodeId));
  const hasOutput = run.output !== null;

  return {
    rows,
    status: run.status,
    // an error stops the walk short; surface it as a bad outcome regardless of the last status.
    tone: run.error ? "bad" : simTone(run.status),
    reachedCount: reached.size,
    error: run.error ?? null,
    outputJson: hasOutput ? JSON.stringify(run.output, null, 2) : null,
  };
}
