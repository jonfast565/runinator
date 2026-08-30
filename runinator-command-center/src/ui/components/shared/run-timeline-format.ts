import type { WorkflowNodeRun } from "../../../core/domain/models";

const FAILED_STATUSES = new Set(["failed", "timed_out"]);

const stepTimestampFormatter = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  fractionalSecondDigits: 3,
});

function stepTime(node: WorkflowNodeRun): string | null {
  const timestamps = [node.started_at, node.created_at];
  return (
    timestamps.find((value) => typeof value === "string" && Number.isFinite(Date.parse(value))) ??
    null
  );
}

function timelineSequence(node: WorkflowNodeRun): number | null {
  const value = node.state?.timeline_sequence;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** The time at which this step began, or was queued when it has not started yet. */
export function stepTimestamp(node: WorkflowNodeRun): string {
  const value = stepTime(node);

  if (!value) {
    return "";
  }

  const timestamp = new Date(value);
  return Number.isNaN(timestamp.getTime()) ? value : stepTimestampFormatter.format(timestamp);
}

/** Ascending execution order, with a stable ID fallback for simultaneous or unknown times. */
export function compareStepsAscending(left: WorkflowNodeRun, right: WorkflowNodeRun): number {
  const leftTime = Date.parse(stepTime(left) ?? "");
  const rightTime = Date.parse(stepTime(right) ?? "");

  if (Number.isFinite(leftTime) && Number.isFinite(rightTime) && leftTime !== rightTime) {
    return leftTime - rightTime;
  }

  if (Number.isFinite(leftTime) !== Number.isFinite(rightTime)) {
    return Number.isFinite(leftTime) ? -1 : 1;
  }

  const leftSequence = timelineSequence(left);
  const rightSequence = timelineSequence(right);

  if (leftSequence !== null && rightSequence !== null && leftSequence !== rightSequence) {
    return leftSequence - rightSequence;
  }

  if (leftSequence !== null || rightSequence !== null) {
    return leftSequence !== null ? -1 : 1;
  }

  // UUIDv7 IDs preserve the finer ordering when two events share a timestamp.
  return left.id.localeCompare(right.id);
}

export function isFailedNode(node: WorkflowNodeRun): boolean {
  return FAILED_STATUSES.has(node.status);
}

export function timelineDotClass(status: string): string {
  const base = "relative z-[1] mt-[7px] size-[11px] rounded-full shadow-[0_0_0_2px_var(--surface)]";

  if (status === "succeeded") {
    return `${base} bg-success-fg`;
  }

  if (status === "failed" || status === "timed_out") {
    return `${base} bg-danger`;
  }

  if (status === "running" || status === "retrying") {
    return `${base} run-timeline-dot status-running bg-accent`;
  }

  if (status === "waiting" || status === "queued" || status === "debug_paused") {
    return `${base} bg-warn`;
  }

  return `${base} bg-border-strong`;
}

export interface TimelineProvenanceTag {
  id: "entered" | "effect_receipt";
  label: string;
  title: string;
}

/** Durable records represented by one projected timeline row. */
export function timelineProvenanceTags(node: WorkflowNodeRun): TimelineProvenanceTag[] {
  const tags: TimelineProvenanceTag[] = [];

  if (typeof node.state?.node_entered_journal_id === "string") {
    tags.push({
      id: "entered",
      label: "entered",
      title: "The workflow journal recorded this node entry.",
    });
  }

  if (typeof node.state?.effect_receipt_id === "string") {
    tags.push({
      id: "effect_receipt",
      label: "effect receipt",
      title: "A durable effect receipt recorded this execution and its result.",
    });
  }

  return tags;
}

export function previewOf(node: WorkflowNodeRun): string {
  const output = node.output_json;

  if (output === undefined || output === null) {
    return "";
  }

  const text = typeof output === "string" ? output : JSON.stringify(output);
  const oneLine = text.replace(/\s+/g, " ").trim();

  if (!oneLine || oneLine === "{}" || oneLine === '""') {
    return "";
  }

  return oneLine.length > 140 ? `${oneLine.slice(0, 140)}…` : oneLine;
}

export function outputText(node: WorkflowNodeRun): string {
  const output = node.output_json;

  if (output === undefined || output === null) {
    return "";
  }

  if (typeof output === "object" && Object.keys(output).length === 0) {
    return "";
  }

  return JSON.stringify(output, null, 2);
}

export function formatMs(ms: number): string {
  if (ms < 1000) {
    return `${String(ms)}ms`;
  }

  const seconds = ms / 1000;

  if (seconds < 60) {
    return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remSec = Math.round(seconds % 60);
  return remSec === 0 ? `${String(minutes)}m` : `${String(minutes)}m ${String(remSec)}s`;
}
