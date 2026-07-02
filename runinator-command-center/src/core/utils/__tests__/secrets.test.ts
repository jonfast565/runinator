import { describe, expect, it } from "vitest";
import { parseSecretRef, secretRef, secretRefLabel } from "../secrets";

describe("secret utils", () => {
  it("round trips secret references", () => {
    const value = secretRef("github", "token/main");
    expect(value).toBe("secret://github/token%2Fmain");
    expect(parseSecretRef(value)).toEqual({ scope: "github", name: "token/main" });
    expect(secretRefLabel(value)).toBe("github/token/main");
  });

  it("ignores non-secret values", () => {
    expect(parseSecretRef("plain text")).toBeNull();
    expect(secretRefLabel("plain text")).toBe("");
  });
});
