import type { JsonRecord, WorkflowNodeRun } from "../types/models";

export type ApprovalAction = "approve" | "reject";

export function isApprovalWaitingStatus(status: unknown): boolean {
  return ["waiting", "approval_required", "pending"].includes(normalizeStatus(status));
}

export function approvalIdFromNodeRun(nodeRun: WorkflowNodeRun): number {
  return positiveNumber(nodeRun.state?.approval_id) || positiveNumber(nodeRun.output_json?.approval_id) || positiveNumber(nodeRun.state?.approval?.id);
}

export function selectWorkflowApprovalRecord(records: JsonRecord[], workflowRunId: number, nodeId: string): JsonRecord | null {
  const matches = records.filter((record) => positiveNumber(record.id) > 0 && positiveNumber(record.workflow_run_id) === workflowRunId && String(record.node_id ?? "") === nodeId);
  matches.sort((left, right) => approvalRecordRank(left) - approvalRecordRank(right) || positiveNumber(right.id) - positiveNumber(left.id));
  return matches[0] ?? null;
}

export function positiveNumber(value: unknown): number {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : 0;
}

function approvalRecordRank(record: JsonRecord): number {
  return isApprovalWaitingStatus(record.status) ? 0 : 1;
}

function normalizeStatus(status: unknown): string {
  return String(status ?? "")
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}
