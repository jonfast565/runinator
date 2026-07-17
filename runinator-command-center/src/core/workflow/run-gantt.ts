import type { WorkflowNodeRun, WorkflowRunDetail } from "../domain/models";

// statuses that mean a node run has settled; anything else is still in flight and its bar counts up.
const TERMINAL = new Set(["succeeded", "failed", "timed_out", "canceled"]);

// a minimum bar width (in axis %) so that instant control nodes still render as a visible tick.
const MIN_BAR_PCT = 0.6;

/** one horizontal row in the proportional run timeline. All positions are percentages of the shared
 * time axis (0–100), ready to drop straight into inline `left`/`width` styles. */
export interface GanttRow {
  id: string;
  nodeId: string;
  status: string;
  attempt: number;
  /** left edge of the queued/parked segment (created_at → started_at). */
  waitLeftPct: number;
  waitWidthPct: number;
  /** left edge and width of the active segment (started_at → finished_at/now). */
  barLeftPct: number;
  barWidthPct: number;
  /** active duration in ms (0 for an instant node). */
  durationMs: number;
  /** queued/parked time before the node became active, in ms. */
  waitMs: number;
  /** true while the node is still in flight (bar counts up to `now`). */
  running: boolean;
  /** true for the run's bottleneck node — the longest active segment (the critical path driver). */
  critical: boolean;
}

export interface GanttTick {
  pct: number;
  label: string;
}

export interface GanttLayout {
  rows: GanttRow[];
  /** total run wall-clock in ms (axis span). */
  totalMs: number;
  ticks: GanttTick[];
  /** node run id of the bottleneck, or null when nothing has a measurable duration. */
  bottleneckId: string | null;
  /** node id (authored id) of the bottleneck, for labelling. */
  bottleneckNodeId: string | null;
}

function parseMs(value: string | null | undefined): number | null {
  if (!value) {
    return null;
  }

  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

/** node start = when it went active (started_at), falling back to when it was created. */
function nodeStart(node: WorkflowNodeRun): number | null {
  return parseMs(node.started_at) ?? parseMs(node.created_at);
}

/** node end = finished_at, or `now` while still in flight, or its start for an instant node. */
function nodeEnd(node: WorkflowNodeRun, now: number): number {
  const finished = parseMs(node.finished_at);

  if (finished !== null) {
    return finished;
  }

  const start = nodeStart(node) ?? now;
  return isRunning(node) ? Math.max(now, start) : start;
}

function isRunning(node: WorkflowNodeRun): boolean {
  return !node.finished_at && !TERMINAL.has(node.status);
}

/** format a millisecond duration the same way the vertical timeline does (ms → s → m). */
export function formatDuration(ms: number): string {
  if (ms < 1000) {
    return `${String(Math.max(0, Math.round(ms)))}ms`;
  }

  const seconds = ms / 1000;

  if (seconds < 60) {
    return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remSec = Math.round(seconds % 60);
  return remSec === 0 ? `${String(minutes)}m` : `${String(minutes)}m ${String(remSec)}s`;
}

/** build a proportional Gantt layout from a run's persisted node timing. Pure: `now` is injected so
 * the caller controls the live clock. Rows are ordered by start time; the longest active segment is
 * flagged as the critical-path bottleneck. */
export function buildGanttLayout(
  detail: WorkflowRunDetail | null,
  now: number,
): GanttLayout {
  const empty: GanttLayout = {
    rows: [],
    totalMs: 0,
    ticks: [],
    bottleneckId: null,
    bottleneckNodeId: null,
  };

  const nodes = detail?.nodes ?? [];

  if (nodes.length === 0) {
    return empty;
  }

  // axis bounds: earliest node/run start to the latest end (or `now` if the run is still going).
  const runStart = parseMs(detail?.run.started_at);
  const runFinish = parseMs(detail?.run.finished_at);
  let t0 = runStart ?? Number.POSITIVE_INFINITY;
  let t1 = runFinish ?? Number.NEGATIVE_INFINITY;

  for (const node of nodes) {
    const start = nodeStart(node);
    const created = parseMs(node.created_at);
    const lower = Math.min(start ?? Number.POSITIVE_INFINITY, created ?? Number.POSITIVE_INFINITY);

    if (Number.isFinite(lower)) {
      t0 = Math.min(t0, lower);
    }

    t1 = Math.max(t1, nodeEnd(node, now));
  }

  if (!Number.isFinite(t0)) {
    return empty;
  }

  if (!Number.isFinite(t1) || t1 <= t0) {
    t1 = t0 + 1;
  }

  const span = t1 - t0;

  const pct = (value: number) => {
    const clamped = Math.min(Math.max(value, t0), t1);
    return ((clamped - t0) / span) * 100;
  };

  const ordered = [...nodes].sort((left, right) => {
    const leftStart = nodeStart(left) ?? parseMs(left.created_at) ?? 0;
    const rightStart = nodeStart(right) ?? parseMs(right.created_at) ?? 0;

    if (leftStart !== rightStart) {
      return leftStart - rightStart;
    }

    return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  });

  const rows: GanttRow[] = ordered.map((node) => {
    const created = parseMs(node.created_at) ?? nodeStart(node) ?? t0;
    const start = nodeStart(node) ?? created;
    const end = nodeEnd(node, now);
    const barLeftPct = pct(start);
    const waitLeftPct = pct(created);

    return {
      id: node.id,
      nodeId: node.node_id,
      status: node.status,
      attempt: node.attempt,
      waitLeftPct,
      waitWidthPct: Math.max(0, pct(start) - waitLeftPct),
      barLeftPct,
      barWidthPct: Math.max(MIN_BAR_PCT, pct(end) - barLeftPct),
      durationMs: Math.max(0, end - start),
      waitMs: Math.max(0, start - created),
      running: isRunning(node),
      critical: false,
    };
  });

  // the bottleneck is the longest measurable active segment — the critical-path driver in a
  // single-cursor run. Derived from the built rows so the value type stays a plain GanttRow.
  let bottleneck: GanttRow | null = null;

  for (const row of rows) {
    if (row.durationMs > 0 && (bottleneck === null || row.durationMs > bottleneck.durationMs)) {
      bottleneck = row;
    }
  }

  if (bottleneck !== null) {
    bottleneck.critical = true;
  }

  const ticks: GanttTick[] = [0, 25, 50, 75, 100].map((p) => ({
    pct: p,
    label: formatDuration((span * p) / 100),
  }));

  return {
    rows,
    totalMs: span,
    ticks,
    bottleneckId: bottleneck?.id ?? null,
    bottleneckNodeId: bottleneck?.nodeId ?? null,
  };
}
