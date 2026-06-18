import { describe, expect, it } from "vitest";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { isWorkflowExpressionValue, workflowExpressionCompletionSource } from "../workflow-expression-completion";

describe("workflow expression detection", () => {
  it("recognizes every expression operator emitted by WDL/runtime expressions", () => {
    for (const key of ["$ref", "$concat", "$coalesce", "$literal", "$to_string", "$to_json_string", "$node"]) {
      expect(isWorkflowExpressionValue({ [key]: key === "$concat" || key === "$coalesce" ? [] : null })).toBe(true);
    }
  });

  it("does not treat plain WDL object literals as whole-value expressions", () => {
    expect(isWorkflowExpressionValue({ value: { "$ref": { params: ["name"] } }, equals: "prod" })).toBe(false);
  });

  it("offers saveable literal and helper completions in expression editors", async () => {
    const state = EditorState.create({ doc: "" });
    const result = await workflowExpressionCompletionSource(() => undefined)(new CompletionContext(state, 0, true));
    const labels = result?.options.map((option) => option.label) ?? [];

    expect(labels).toEqual(expect.arrayContaining([
      "params",
      "config",
      "secret",
      "true",
      "false",
      "null",
      "string",
      "json",
      "concat",
      "coalesce",
      "object",
      "array"
    ]));
  });
});
