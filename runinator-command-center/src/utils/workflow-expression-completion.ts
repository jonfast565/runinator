import { snippet, type Completion, type CompletionContext, type CompletionResult, type CompletionSource } from "@codemirror/autocomplete";
import { nodeOutputReferences, paramsReferences, type WorkflowExpressionEditorContext } from "./workflow-references";
export { isWorkflowExpressionValue } from "./wdl-expression";
export type { WorkflowExpressionEditorContext } from "./workflow-references";

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
    snippetCompletion("params", "params.${field}", "variable", "workflow parameter reference"),
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

  // schema-derived parameter fields and prior node outputs share the picker's reference catalog.
  for (const ref of paramsReferences(context?.workflowInputType ?? null)) {
    completions.push(snippetCompletion(ref.insert, ref.insert, "variable", ref.type));
  }

  for (const ref of nodeOutputReferences(context)) {
    completions.push(snippetCompletion(ref.insert, ref.insert, "variable", ref.type));
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
