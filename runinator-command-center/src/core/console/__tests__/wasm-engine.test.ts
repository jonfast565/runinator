import { describe, expect, it } from "vitest";

import { ctlCatalog, ctlComplete, ctlIsSubmittable, ctlParse } from "../wasm-engine";

describe("runinatorctl WASM engine", () => {
  it("derives its catalog from the native clap tree", () => {
    const names = ctlCatalog().map((command) => command.path.join(" "));
    expect(names).toContain("workflows list");
    expect(names).toContain("replicas samples");
    expect(names).toContain("run workflow");
  });

  it("parses and validates a command", () => {
    expect(ctlParse(":runs list --status running --json")).toMatchObject({
      kind: "command",
      path: ["runs", "list"],
      flags: { status: ["running"] },
      json: true,
    });
    expect(() => ctlParse(":runs list --stauts running")).toThrow(/stauts/);
  });

  it("provides clap-derived completion", () => {
    expect(ctlComplete(":workfl").options).toEqual(["workflows"]);
    expect(ctlComplete(":settings list --kind ").options).toEqual(["config", "secret"]);
  });

  it("decides when multiline REXRAP is complete", () => {
    expect(ctlIsSubmittable('workflow "x" v1 {')).toBe(false);
    expect(ctlIsSubmittable('workflow "x" v1 {\n  yield { value: 1 }\n}')).toBe(true);
  });
});
