import type { WorkflowNodeRun } from "../types/models";

export function isInputWaitingStatus(status: unknown): boolean {
  return ["waiting", "input_required", "pending"].includes(normalizeStatus(status));
}

export function inputValueFromNodeRun(nodeRun: WorkflowNodeRun): unknown {
  return nodeRun.output_json ?? nodeRun.state?.input ?? null;
}

function normalizeStatus(status: unknown): string {
  return String(status ?? "")
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}
