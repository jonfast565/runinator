export function isBadStatus(status?: unknown) {
  return (
    typeof status === "string" &&
    ["blocked", "failed", "rejected", "timed_out", "canceled"].includes(status)
  );
}

export function isGoodStatus(status?: unknown) {
  return typeof status === "string" && ["approved", "succeeded", "passed", "open"].includes(status);
}

export function isTerminalWorkflowRunStatus(status?: string) {
  return ["succeeded", "failed", "timed_out", "canceled"].includes(status ?? "");
}

// a run (workflow or pipeline — they share the status vocabulary) that is still doing work: not yet
// settled into a terminal state. used by the runs monitors to count active runs and gate cancel.
export function isActiveRunStatus(status?: string): boolean {
  return !isTerminalWorkflowRunStatus(status);
}

// count the runs still in flight in a list of run-like records.
export function countActiveRuns(runs: { status: string }[]): number {
  return runs.filter((run) => isActiveRunStatus(run.status)).length;
}

// a node whose current run has settled: succeeded, failed, or otherwise not
// still doing work. used to freeze the flow animation on the completed trail.
// a node not in this set (running/waiting/queued/pending or not yet reached)
// is treated as incomplete and keeps animating.
export function isCompletedNodeStatus(status?: unknown) {
  return (
    typeof status === "string" &&
    [
      "succeeded",
      "passed",
      "approved",
      "failed",
      "rejected",
      "timed_out",
      "canceled",
      "blocked",
      "skipped",
    ].includes(status)
  );
}

export function statusBadgeClass(status?: string) {
  if (isBadStatus(status)) {
    return "status-failed";
  }

  if (isGoodStatus(status)) {
    return "status-succeeded";
  }

  if (status === "running") {
    return "status-running";
  }

  if (
    [
      "queued",
      "waiting",
      "approval_required",
      "input_required",
      "debug_paused",
      "paused",
      "pending",
    ].includes(status ?? "")
  ) {
    return "status-waiting";
  }

  return "status-muted";
}

export function statusClassForNode(status?: string) {
  if (["succeeded", "passed", "approved"].includes(status ?? "")) {
    return "node-success";
  }

  if (["failed", "rejected", "timed_out", "canceled", "blocked"].includes(status ?? "")) {
    return "node-danger";
  }

  if (status === "running") {
    return "node-running";
  }

  if (
    ["waiting", "approval_required", "input_required", "approval-required", "pending"].includes(
      status ?? "",
    )
  ) {
    return "node-waiting";
  }

  if (["debug_paused", "paused", "queued"].includes(status ?? "")) {
    return "node-warning";
  }

  if (status) {
    return "node-active";
  }

  return "";
}
