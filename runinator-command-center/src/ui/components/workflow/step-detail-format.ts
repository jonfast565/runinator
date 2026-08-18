import type { JsonRecord, RuninatorType } from "../../../core/domain/models";
import { nodeRefId } from "../../../core/workflow";
import { describeRetryPolicy } from "../../../core/workflow/retry";
import { displayValue } from "../../../core/utils/values";

export function conditionLabel(value: unknown): string {
  if (!isRecord(value)) {
    return valueLabel(value);
  }

  if ("equals" in value) {
    return `${valueLabel(value.value)} equals ${valueLabel(value.equals)}`;
  }

  if ("not_equals" in value) {
    return `${valueLabel(value.value)} not equals ${valueLabel(value.not_equals)}`;
  }

  if ("exists" in value) {
    return `${valueLabel(value.exists)} exists`;
  }

  return valueLabel(value);
}

export function refLabel(value: unknown): string {
  return nodeRefId(value) ?? "-";
}

// render a runinator type into a short readable signature (e.g. array<string>, map<integer>).
export function renderType(ty: RuninatorType | null | undefined): string {
  if (!ty) {
    return "any";
  }

  switch (ty.type) {
    case "array":
      return `array<${renderType(ty.items)}>`;
    case "map":
      return `map<${renderType(ty.values)}>`;
    case "struct":
      return "struct";
    case "union":
      return ty.variants.map(renderType).join(" | ");
    case "enum":
      return `enum[${ty.values.map((value) => JSON.stringify(value)).join(", ")}]`;
    case "range":
      return `${renderType(ty.base)} range ${String(ty.min ?? "")}..${String(ty.max ?? "")}`;
    default:
      return ty.type;
  }
}

export function valueLabel(value: unknown): string {
  if (value == null) {
    return "-";
  }

  if (typeof value === "string") {
    return value || "-";
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  if (Array.isArray(value)) {
    return value.length ? value.map(valueLabel).join(", ") : "empty list";
  }

  if (!isRecord(value)) {
    return displayValue(value);
  }

  const nodeRef = nodeRefId(value);

  if (nodeRef) {
    return `node ${nodeRef}`;
  }

  if (isRecord(value.$ref)) {
    return refExpressionLabel(value.$ref);
  }

  if (Array.isArray(value.$concat)) {
    return `concat ${String(value.$concat.length)} part${value.$concat.length === 1 ? "" : "s"}`;
  }

  const entries = Object.entries(value);

  if (entries.length === 0) {
    return "none";
  }

  return (
    entries
      .slice(0, 4)
      .map(([key, nested]) => `${key}: ${valueLabel(nested)}`)
      .join("; ") + (entries.length > 4 ? `; +${String(entries.length - 4)} more` : "")
  );
}

export function refExpressionLabel(ref: JsonRecord): string {
  for (const source of ["params", "prev", "workflow", "output"]) {
    if (Array.isArray(ref[source])) {
      return `${source}.${ref[source].join(".")}`;
    }
  }

  if (typeof ref.node === "string" && Array.isArray(ref.output)) {
    return `${ref.node}.output.${ref.output.join(".")}`;
  }

  return "reference";
}

export function waitSummary(wait: unknown): string {
  const record = isRecord(wait) ? wait : {};

  if (record.seconds) {
    return `Wait ${displayValue(record.seconds)}s`;
  }

  if (record.until) {
    return `Wait until ${displayValue(record.until)}`;
  }

  return "Wait for external timing";
}

export function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

/** a `label: value` pair in one of the detail grids. */
export interface MetaEntry {
  label: string;
  value: string;
  mono?: boolean;
}

/**
 * the node's retry policy as a sentence rather than an attempt count. the backoff, cap, jitter, and
 * retry class together decide what actually happens, and four numbers in a grid do not say it.
 */
export function retrySummary(node: JsonRecord): string {
  const retry = isRecord(node.retry) ? node.retry : {};
  return describeRetryPolicy({
    max_attempts: Number(retry.max_attempts ?? node.max_attempts ?? 1),
    backoff_base_seconds: Number(retry.backoff_base_seconds ?? 1),
    backoff_max_seconds: Number(retry.backoff_max_seconds ?? 300),
    jitter: retry.jitter === true,
    retry_on: displayValue(retry.retry_on) || "any",
  });
}

/**
 * the action band: provider, function, both deadlines, and the retry reading.
 *
 * the two timeouts are listed separately on purpose — `action.timeout_seconds` is the worker's call
 * deadline and `node.timeout_seconds` is the reducer's node deadline, and collapsing them into one
 * "Timeout" row is what made the editor write an edit to the field nobody was looking at.
 */
export function actionMetaRows(
  node: JsonRecord,
  provider: string,
  actionFunction: string,
): MetaEntry[] {
  const call = isRecord(node.action) ? node.action.timeout_seconds : undefined;
  const nodeTimeout = node.timeout_seconds;
  return [
    { label: "Provider", value: provider || "—", mono: true },
    { label: "Function", value: actionFunction || "—", mono: true },
    { label: "Call Timeout", value: call != null ? `${displayValue(call)}s` : "default" },
    {
      label: "Node Timeout",
      value: nodeTimeout != null ? `${displayValue(nodeTimeout)}s` : "none",
    },
    { label: "Retry", value: retrySummary(node) },
  ];
}

/** the compensating call's band, empty when the node declares none. */
export function compensationRows(node: JsonRecord): MetaEntry[] {
  const compensation = node.compensation;

  if (!isRecord(compensation)) {
    return [];
  }

  const configured = isRecord(compensation.configuration)
    ? Object.keys(compensation.configuration)
    : [];
  const timeout = compensation.timeout_seconds;
  return [
    { label: "Provider", value: displayValue(compensation.provider) || "—", mono: true },
    { label: "Function", value: displayValue(compensation.function) || "—", mono: true },
    { label: "Timeout", value: timeout != null ? `${displayValue(timeout)}s` : "default" },
    { label: "Parameters", value: configured.length ? configured.join(", ") : "none", mono: true },
  ];
}
