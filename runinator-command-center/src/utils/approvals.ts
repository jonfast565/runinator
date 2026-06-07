import type { JsonRecord, WorkflowNodeRun } from "../types/models";

export type ApprovalAction = "approve" | "reject";

export function isApprovalWaitingStatus(status: unknown): boolean {
  return ["waiting", "approval_required", "pending"].includes(normalizeStatus(status));
}

export function approvalIdFromNodeRun(nodeRun: WorkflowNodeRun): string | null {
  return nonEmptyString(nodeRun.state?.approval_id) ?? nonEmptyString(nodeRun.output_json?.approval_id) ?? nonEmptyString(nodeRun.state?.approval?.id);
}

export function selectWorkflowApprovalRecord(records: JsonRecord[], workflowRunId: string, nodeId: string): JsonRecord | null {
  const matches = records.filter((record) => nonEmptyString(record.id) && String(record.workflow_run_id ?? "") === workflowRunId && String(record.node_id ?? "") === nodeId);
  matches.sort((left, right) => approvalRecordRank(left) - approvalRecordRank(right) || recordTime(right) - recordTime(left));
  return matches[0] ?? null;
}

export function nonEmptyString(value: unknown): string | null {
  const text = typeof value === "string" ? value.trim() : "";
  return text ? text : null;
}

function approvalRecordRank(record: JsonRecord): number {
  return isApprovalWaitingStatus(record.status) ? 0 : 1;
}

function recordTime(record: JsonRecord): number {
  const raw = record.updated_at ?? record.created_at;
  const time = typeof raw === "string" ? Date.parse(raw) : NaN;
  return Number.isFinite(time) ? time : 0;
}

function normalizeStatus(status: unknown): string {
  return String(status ?? "")
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}
