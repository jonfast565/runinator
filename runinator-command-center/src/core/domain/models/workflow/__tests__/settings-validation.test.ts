import { describe, expect, it } from "vitest";
import { workflowSettingsErrors } from "../settings-validation";

describe("workflowSettingsErrors", () => {
  const valid = {
    name: "Release workflow",
    namespace: "acme.delivery",
    key: "release_train",
    version: "1.0.0",
  };

  it("accepts a complete workflow identity", () => {
    expect(workflowSettingsErrors(valid)).toEqual({
      name: "",
      namespace: "",
      key: "",
      version: "",
    });
  });

  it("keeps field errors independent", () => {
    expect(workflowSettingsErrors({ ...valid, name: "", version: "one" })).toEqual({
      name: "Name is required.",
      namespace: "",
      key: "",
      version: "Use a semantic version with major, minor, and patch numbers, for example 1.0.0.",
    });
  });
});
