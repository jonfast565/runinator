import type {
  ActionResultMetadata,
  JsonRecord,
  ProviderMetadata,
  WorkflowNodeKind,
} from "../domain/models";
import { asJsonRecord, isJsonRecord } from "../domain/json";
import { displayValue, isBlankValue } from "../utils/values";
import { addableNodeKinds, findNodeKindMetadata } from "./catalog-registry";
import { getAtLocation } from "./field-location";

export function workflowNodeKind(value: unknown): WorkflowNodeKind {
  return typeof value === "string" &&
    ["start", ...addableNodeKinds(), "end", "fail"].includes(value)
    ? (value as WorkflowNodeKind)
    : "action";
}

export function workflowNodeActionConfig(
  node: JsonRecord,
): { provider: string; action: string } {
  const action = isJsonRecord(node.action) ? node.action : {};
  return {
    provider: displayValue(action.provider),
    action: displayValue(action.function),
  };
}

export function workflowNodeActionInputs(node: JsonRecord): JsonRecord {
  const action = isJsonRecord(node.action) ? node.action : null;
  const configuration = action && isJsonRecord(action.configuration) ? action.configuration : {};
  const parameters = isJsonRecord(node.parameters) ? node.parameters : {};
  return { ...configuration, ...parameters };
}

function isExpressionValue(value: unknown): boolean {
  if (!isJsonRecord(value)) {
    return false;
  }

  return [
    "$ref",
    "$concat",
    "$coalesce",
    "$literal",
    "$to_string",
    "$to_json_string",
    "$node",
  ].some((key) => key in value);
}

export function isEmptyInputValue(value: unknown): boolean {
  return !isExpressionValue(value) && isBlankValue(value);
}

export function workflowNodeResultMetadata(
  node: JsonRecord,
  providers: ProviderMetadata[],
): ActionResultMetadata[] {
  const config = workflowNodeActionConfig(node);

  if (!config.provider || !config.action) {
    return [];
  }

  const provider = providers.find((item) => item.name === config.provider);
  const action = provider?.actions.find((item) => item.function_name === config.action);
  return action?.results ?? [];
}

export function nodeDisplayName(node: JsonRecord, id: string): string {
  const name = typeof node.name === "string" ? node.name.trim() : "";
  return name || id;
}

function describeValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }

  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  if (isJsonRecord(value)) {
    if (typeof value.$node === "string") {
      return `→ ${value.$node}`;
    }

    if (isJsonRecord(value.$ref)) {
      const [source, path] = Object.entries(value.$ref)[0] ?? [];
      const segments = Array.isArray(path) ? path.join(".") : "";
      return `\${${source}${segments ? `.${segments}` : ""}}`;
    }

    if ("$value" in value) {
      return describeValue(value.$value);
    }
  }

  if (Array.isArray(value)) {
    return value.length === 0
      ? "[]"
      : `[${String(value.length)} item${value.length === 1 ? "" : "s"}]`;
  }

  try {
    const json = JSON.stringify(value);
    return json.length > 60 ? `${json.slice(0, 57)}…` : json;
  } catch {
    return "…";
  }
}

export function nodeSummary(node: JsonRecord, subflowNames?: Map<string, string>): string {
  const kind = workflowNodeKind(node.kind);

  if (kind === "action") {
    const config = workflowNodeActionConfig(node);

    if (!config.provider) {
      return "Unconfigured action";
    }

    return config.action ? `${config.provider}.${config.action}` : config.provider;
  }

  if (kind === "subflow") {
    const subflowId = node.subflow_id != null ? displayValue(node.subflow_id) : "";
    return subflowNames?.get(subflowId) ?? `Workflow ${subflowId || "-"}`;
  }

  if (kind === "start") {return "Start";}
  if (kind === "end") {return "Success";}
  if (kind === "fail") {return "Workflow failure";}

  const metadata = findNodeKindMetadata(kind);
  const fields = metadata?.fields
    .map((field) => describeValue(getAtLocation(node, field.location)))
    .filter(Boolean)
    .slice(0, 3);
  return fields?.length ? fields.join(" · ") : (metadata?.label ?? kind);
}

export function approvalPrompt(node: JsonRecord, state?: JsonRecord): string | undefined {
  if (workflowNodeKind(node.kind) !== "approval") {
    return undefined;
  }

  return (
    describeValue(
      state?.prompt ??
        asJsonRecord(state?.approval).prompt ??
        asJsonRecord(node.parameters).prompt,
    ) || "Approval required"
  );
}

export function inputPrompt(node: JsonRecord, state?: JsonRecord): string | undefined {
  if (workflowNodeKind(node.kind) !== "input") {
    return undefined;
  }

  return (
    describeValue(asJsonRecord(state?.input).prompt ?? asJsonRecord(node.parameters).prompt) ||
    "Input required"
  );
}
