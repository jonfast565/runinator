import type { WorkflowNodeRun } from "../types/models";
import { displayValue } from "./values";

export function isInputWaitingStatus(status: unknown): boolean {
  return ["waiting", "input_required", "pending"].includes(normalizeStatus(status));
}

export function inputValueFromNodeRun(nodeRun: WorkflowNodeRun): unknown {
  return nodeRun.output_json ?? nodeRun.state?.input ?? null;
}

function normalizeStatus(status: unknown): string {
  return displayValue(status)
    .trim()
    .toLowerCase()
    .replaceAll("-", "_");
}
