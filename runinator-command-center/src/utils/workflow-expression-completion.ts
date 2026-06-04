import { snippet, type Completion, type CompletionContext, type CompletionResult, type CompletionSource } from "@codemirror/autocomplete";
import type { JsonRecord, ProviderMetadata, RuninatorType } from "../types/models";
import { workflowNodeActionConfig } from "./workflows";
export { isWorkflowExpressionValue } from "./wdl-expression";

export interface WorkflowExpressionEditorContext {
  workflowInputType?: RuninatorType | null;
  nodes?: JsonRecord[];
  currentNodeId?: string | null;
  providers?: ProviderMetadata[];
}

export function workflowExpressionCompletionSource(context: () => WorkflowExpressionEditorContext | undefined): CompletionSource {
  return (completionContext: CompletionContext): CompletionResult | null => {
    const word = completionContext.matchBefore(/[\w./-]+/);
    if (!completionContext.explicit && !word) return null;
    return {
      from: word?.from ?? completionContext.pos,
      options: expressionCompletions(context()),
      validFor: /^[\w./-]*$/
    };
  };
}

function expressionCompletions(context?: WorkflowExpressionEditorContext): Completion[] {
  const completions: Completion[] = [
    snippetCompletion("input", "input.${field}", "variable", "workflow input reference"),
    snippetCompletion("prev", "prev.${field}", "variable", "previous node output reference"),
    snippetCompletion("run", "run.${field}", "variable", "workflow state reference"),
    snippetCompletion("config", "config.${field}", "variable", "configuration reference"),
    snippetCompletion("secret", "secret.${scope}.${name}", "variable", "secret reference"),
    snippetCompletion("string", "string(${value})", "function", "convert scalar to string"),
    snippetCompletion("json", "json(${value})", "function", "convert object or array to JSON text"),
    snippetCompletion("concat", "${left} ++ ${right}", "function", "string concatenation"),
    snippetCompletion("coalesce", "${left} ?? ${right}", "function", "first non-null value"),
    snippetCompletion("object", "{ ${key}: ${value} }", "constant", "object literal"),
    snippetCompletion("array", "[${items}]", "constant", "array literal")
  ];

  for (const field of inputFields(context?.workflowInputType ?? null)) {
    completions.push(snippetCompletion(`input.${field.path}`, `input.${field.path}`, "variable", field.type));
  }

  for (const ref of nodeOutputRefs(context)) {
    completions.push(snippetCompletion(`${ref.node}.${ref.field}`, `${ref.node}.${ref.field}`, "variable", ref.type));
  }

  return completions;
}

function snippetCompletion(label: string, apply: string, type: Completion["type"], detail: string): Completion {
  return {
    label,
    type,
    detail,
    apply: snippet(apply)
  };
}

function inputFields(ty: RuninatorType | null): Array<{ path: string; segments: string[]; type: string }> {
  if (!ty || ty.type !== "struct") return [];
  const fields: Array<{ path: string; segments: string[]; type: string }> = [];
  collectInputFields(ty, [], fields);
  return fields;
}

function collectInputFields(ty: RuninatorType, path: string[], fields: Array<{ path: string; segments: string[]; type: string }>) {
  if (ty.type !== "struct") return;
  for (const [name, field] of Object.entries(ty.fields)) {
    const nextPath = [...path, name];
    fields.push({ path: nextPath.join("."), segments: nextPath, type: describeType(field.ty) });
    collectInputFields(field.ty, nextPath, fields);
  }
}

function nodeOutputRefs(context?: WorkflowExpressionEditorContext): Array<{ node: string; field: string; path: string[]; type: string }> {
  const nodes = context?.nodes ?? [];
  const providers = context?.providers ?? [];
  const refs: Array<{ node: string; field: string; path: string[]; type: string }> = [];
  for (const node of nodes) {
    if (node.kind !== "action" || node.id === context?.currentNodeId) continue;
    const config = workflowNodeActionConfig(node);
    const provider = providers.find((item) => item.name === config.provider);
    const action = provider?.actions.find((item) => item.function_name === config.action);
    for (const result of action?.results ?? []) {
      refs.push({ node: String(node.id), field: result.name, path: [result.name], type: describeType(result.ty) });
    }
  }
  return refs;
}

function describeType(ty: RuninatorType | undefined): string {
  if (!ty) return "any";
  if (ty.type === "array") return `${describeType(ty.items)}[]`;
  if (ty.type === "map") return `map<string, ${describeType(ty.values)}>`;
  if (ty.type === "union") return ty.variants.map(describeType).join(" | ");
  return ty.type;
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
