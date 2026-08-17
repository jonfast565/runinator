// covers the two decisions the prompt makes on a keystroke: does Enter submit, and what does Tab
// offer.

import { describe, expect, it } from "vitest";
import { complete, isSubmittable } from "../prompt";

describe("isSubmittable", () => {
  it("submits a finished expression", () => {
    expect(isSubmittable("1 + 2")).toBe(true);
  });

  it("waits for an unclosed block", () => {
    expect(isSubmittable('workflow "x" v1 {')).toBe(false);
    expect(isSubmittable('workflow "x" v1 {\n  yield { value: 1 }\n}')).toBe(true);
  });

  it("waits for an unclosed quote", () => {
    expect(isSubmittable('"unfinished')).toBe(false);
  });

  it("treats a trailing backslash as a continuation", () => {
    expect(isSubmittable("1 + \\")).toBe(false);
  });

  it("always submits a command, which is never multi-line", () => {
    expect(isSubmittable(":workflows list")).toBe(true);
  });

  it("does not submit an empty line", () => {
    expect(isSubmittable("   ")).toBe(false);
  });
});

describe("complete", () => {
  it("offers nothing for a wdl line", () => {
    expect(complete("1 + ").options).toEqual([]);
    expect(complete("action jira.create").options).toEqual([]);
  });

  it("offers verbs for a bare colon", () => {
    const { options } = complete(":");
    expect(options).toContain("workflows");
    expect(options).toContain("bindings");
  });

  it("narrows to the typed prefix", () => {
    expect(complete(":workfl").options).toEqual(["workflows"]);
  });

  it("offers subcommands after a verb", () => {
    const { options } = complete(":workflows ");
    expect(options).toContain("run");
    expect(options).toContain("rollback");
  });

  it("replaces only the word being typed", () => {
    const line = ":workflows rollb";
    expect(line.slice(complete(line).start)).toBe("rollb");
  });

  it("offers the flags a command accepts", () => {
    const { options } = complete(":runs list --");
    expect(options).toContain("--status");
    expect(options).toContain("--json");
  });

  it("offers the values a flag accepts", () => {
    expect(complete(":settings list --kind ").options).toEqual(["config", "secret"]);
    expect(complete(":settings list --kind sec").options).toEqual(["secret"]);
  });

  it("hints at a value it cannot complete", () => {
    // a flag whose values are open-ended still says what it wants.
    expect(complete(":replicas samples --since-seconds ").hint).toBe("--since-seconds <N>");
    // so does a positional, once the command path is finished.
    expect(complete(":workflows show ").hint).toBe("<workflow>");
  });

  it("hints nothing once a command has all its arguments", () => {
    expect(complete(":workflows list ")).toMatchObject({ options: [], hint: undefined });
  });
});
