import type { JsonRecord, RuninatorType, WorkflowDefinition, WorkflowEdgeEditorDraft, WorkflowTrigger, WorkflowTriggerKind } from "../../types/models";
import { pretty } from "../../utils/format";
import { isBlankValue } from "../../utils/values";
import { nodeRef, nodeRefId, valueRef } from "../../utils/workflows";

export type BranchPolicyName = "all" | "any" | "first_success";
export type SwitchCaseEditor = { match_kind: "equals" | "not_equals" | "exists" | "when"; match_json: string; target: string };

const protectedWorkflowNodeKinds = new Set(["start", "end", "fail"]);

export function nodeRefArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map(nodeRefId).filter((item): item is string => Boolean(item)) : [];
}

export function defaultEdgeEditorDraft(): WorkflowEdgeEditorDraft {
  return {
    edgeId: "",
    source: "",
    target: "",
    optionId: "",
    edgeStyle: "square",
    label: "",
    whenJson: pretty({ value: valueRef("input", ["value"]), equals: true }),
    matchKind: "equals",
    matchJson: pretty(true),
    canEditLabel: false,
    canEditCondition: false,
    canEditSwitchCase: false,
    canMove: false,
    orderIndex: -1,
    orderCount: 0
  };
}

export function branchPolicyName(value: unknown, fallback: BranchPolicyName): BranchPolicyName {
  return value === "all" || value === "any" || value === "first_success" ? value : fallback;
}

export function switchCaseEditor(value: JsonRecord): SwitchCaseEditor {
  const target = nodeRefId(value.target) ?? "";
  if (value.when !== undefined) return { match_kind: "when", match_json: pretty(value.when), target };
  if (value.not_equals !== undefined) return { match_kind: "not_equals", match_json: pretty(value.not_equals), target };
  if (value.exists !== undefined) return { match_kind: "exists", match_json: pretty(Boolean(value.exists)), target };
  return { match_kind: "equals", match_json: pretty(value.equals ?? ""), target };
}

export function newWorkflowTriggerDraft(workflowId: number, kind: WorkflowTriggerKind = "cron"): WorkflowTrigger {
  return {
    id: null,
    workflow_id: workflowId,
    kind,
    enabled: true,
    configuration: defaultTriggerConfiguration(kind),
    next_execution: null,
    blackout_start: null,
    blackout_end: null,
    metadata: {}
  };
}

export function defaultTriggerConfiguration(kind: WorkflowTriggerKind): JsonRecord {
  if (kind === "cron") return { cron: "0 * * * *", parameters: {} };
  return {};
}

// seed a draft input object from the workflow's input struct so declared fields render pre-populated.
export function buildInputSkeleton(ty: RuninatorType | null): JsonRecord {
  if (!ty || ty.type !== "struct") return {};
  const skeleton: JsonRecord = {};
  for (const [name, field] of Object.entries(ty.fields)) {
    skeleton[name] = defaultValueForInputType(field.ty);
  }
  return skeleton;
}

function defaultValueForInputType(ty: RuninatorType): unknown {
  switch (ty.type) {
    case "string":
      return "";
    case "boolean":
      return false;
    case "integer":
    case "number":
      return 0;
    case "array":
      return [];
    case "map":
      return {};
    case "struct":
      return buildInputSkeleton(ty);
    case "union":
      return ty.variants.length ? defaultValueForInputType(ty.variants[0]) : null;
    default:
      return null;
  }
}

export function dateTimeLocalToIso(value: string | null | undefined): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toISOString();
}

export function newWorkflowDraft(): WorkflowDefinition {
  return {
    id: null,
    name: "New Workflow",
    version: 1,
    enabled: true,
    input_type: { type: "struct", fields: {}, additional: { type: "any" } },
    definition: {
      start: "start",
      nodes: [
        { id: "start", kind: "start", transitions: { next: nodeRef("end") } },
        { id: "end", kind: "end" },
        { id: "fail", kind: "fail" }
      ],
      ui: {
        layout: {
          nodes: {
            start: { x: 0, y: 0 },
            end: { x: 270, y: 0 },
            fail: { x: 270, y: 150 }
          }
        }
      }
    }
  };
}

export function boundedIndex(current: number, delta: number, length: number): number {
  if (current < 0) return delta > 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}

// tauri command rejections surface as the serialized CommandError string; fall back to
// String() for native errors.
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

export function formatMaybeDate(value?: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function normalizeNewNodeTargets(node: JsonRecord, endId: string) {
  node.transitions = node.transitions ?? {};
  for (const key of ["next", "on_success", "on_reject"]) {
    if (nodeRefId(node.transitions[key]) === "end") node.transitions[key] = nodeRef(endId);
  }
  if (Array.isArray(node.transitions.branches)) {
    for (const branch of node.transitions.branches) {
      if (nodeRefId(branch.target) === "end") branch.target = nodeRef(endId);
    }
  }
  if (nodeRefId(node.parameters?.target) === "end") node.parameters.target = nodeRef(endId);
  if (nodeRefId(node.parameters?.default) === "end") node.parameters.default = nodeRef(endId);
}

export function validateJsonValueType(value: unknown, ty: RuninatorType | undefined, label: string): string {
  if (!ty || ty.type === "any" || isWorkflowExpression(value)) return "";
  if (ty.type === "null") return value === null ? "" : `${label} must be null`;
  if (ty.type === "string") return typeof value === "string" ? "" : `${label} must be a string`;
  if (ty.type === "boolean") return typeof value === "boolean" ? "" : `${label} must be true or false`;
  if (ty.type === "integer") return typeof value === "number" && Number.isInteger(value) ? "" : `${label} must be an integer`;
  if (ty.type === "number") return typeof value === "number" && !Number.isNaN(value) ? "" : `${label} must be a number`;
  if (ty.type === "array") {
    if (!Array.isArray(value)) return `${label} must be a list`;
    for (let index = 0; index < value.length; index++) {
      const error = validateJsonValueType(value[index], ty.items, `${label}[${index}]`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "map") {
    if (!isJsonRecord(value)) return `${label} must be an object`;
    for (const [key, nested] of Object.entries(value)) {
      const error = validateJsonValueType(nested, ty.values, `${label}.${key}`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "struct") {
    if (!isJsonRecord(value)) return `${label} must be an object`;
    for (const [key, field] of Object.entries(ty.fields)) {
      const nested = value[key];
      if (isBlankValue(nested)) {
        if (field.required) return `${label}.${key} is required`;
        continue;
      }
      const error = validateJsonValueType(nested, field.ty, `${label}.${key}`);
      if (error) return error;
    }
    for (const [key, nested] of Object.entries(value)) {
      if (ty.fields[key]) continue;
      if (!ty.additional) return `${label}.${key} is not allowed`;
      const error = validateJsonValueType(nested, ty.additional, `${label}.${key}`);
      if (error) return error;
    }
    return "";
  }
  if (ty.type === "union") {
    return ty.variants.some((variant) => !validateJsonValueType(value, variant, label))
      ? ""
      : `${label} must match one of ${ty.variants.map((variant) => variant.type).join(", ")}`;
  }
  return "";
}

function isJsonRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isProtectedWorkflowNode(node: JsonRecord | null | undefined): boolean {
  return protectedWorkflowNodeKinds.has(String(node?.kind ?? ""));
}

export function isLockedWorkflowNode(node: JsonRecord | null | undefined): boolean {
  return isProtectedWorkflowNode(node) || node?.locked === true;
}

function isWorkflowExpression(value: unknown): boolean {
  if (!isJsonRecord(value)) return false;
  return ["$ref", "$concat", "$coalesce", "$literal", "$to_string", "$to_json_string", "$node"].some((key) => Object.prototype.hasOwnProperty.call(value, key));
}

export function nextNodePosition(count: number): { x: number; y: number } {
  return { x: ((count - 1) % 4) * 230, y: Math.floor((count - 1) / 4) * 130 };
}
