export function isBadStatus(status?: string) {
  return ["blocked", "failed", "rejected", "timed_out", "canceled"].includes(status ?? "");
}

export function isGoodStatus(status?: string) {
  return ["approved", "succeeded", "passed"].includes(status ?? "");
}

export function statusBadgeClass(status?: string) {
  if (isBadStatus(status)) return "status-failed";
  if (isGoodStatus(status)) return "status-succeeded";
  if (status === "running") return "status-running";
  if (["queued", "waiting", "approval_required"].includes(status ?? "")) return "status-waiting";
  return "status-muted";
}

export function statusClassForNode(status?: string) {
  if (status === "succeeded") return "node-success";
  if (["failed", "timed_out", "canceled"].includes(status ?? "")) return "node-danger";
  if (status === "running") return "node-running";
  if (status) return "node-active";
  return "";
}
