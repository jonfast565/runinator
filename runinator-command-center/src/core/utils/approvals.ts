import type { JsonRecord, WorkflowNodeRun } from "../domain/models";
import { displayValue } from "./values";

export type ApprovalAction = "approve" | "reject";

export function isApprovalWaitingStatus(status: unknown): boolean {
  return ["waiting", "approval_required", "pending"].includes(normalizeStatus(status));
}

export function approvalIdFromNodeRun(nodeRun: WorkflowNodeRun): string | null {
  return (
    nonEmptyString(nodeRun.state?.approval_id) ??
    nonEmptyString((nodeRun.output_json as Record<string, unknown> | undefined)?.approval_id) ??
    nonEmptyString((nodeRun.state?.approval as Record<string, unknown> | undefined)?.id)
  );
}

export function selectWorkflowApprovalRecord(
  records: JsonRecord[],
  workflowRunId: string,
  nodeId: string,
): JsonRecord | null {
  const matches = records.filter(
    (record) =>
      nonEmptyString(record.id) &&
      displayValue(record.workflow_run_id) === workflowRunId &&
      displayValue(record.node_id) === nodeId,
  );
  matches.sort(
    (left, right) =>
      approvalRecordRank(left) - approvalRecordRank(right) || recordTime(right) - recordTime(left),
  );
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
  return displayValue(status)
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}
