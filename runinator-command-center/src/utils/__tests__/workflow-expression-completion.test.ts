import { describe, expect, it } from "vitest";
import { isWorkflowExpressionValue } from "../workflow-expression-completion";

describe("workflow expression detection", () => {
  it("recognizes every expression operator emitted by WDL/runtime expressions", () => {
    for (const key of ["$ref", "$concat", "$coalesce", "$literal", "$to_string", "$to_json_string", "$node"]) {
      expect(isWorkflowExpressionValue({ [key]: key === "$concat" || key === "$coalesce" ? [] : null })).toBe(true);
    }
  });

  it("does not treat plain WDL object literals as whole-value expressions", () => {
    expect(isWorkflowExpressionValue({ value: { "$ref": { input: ["name"] } }, equals: "prod" })).toBe(false);
  });
});
