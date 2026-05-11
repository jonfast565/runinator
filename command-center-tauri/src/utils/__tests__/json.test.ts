import { describe, expect, it } from "vitest";
import { reactive } from "vue";
import { cloneJson, parseObject, parseRequiredObject } from "../json";

describe("json utils", () => {
  it("parses JSON objects", () => {
    expect(parseRequiredObject('{"a":1}')).toEqual({ a: 1 });
  });

  it("rejects arrays as required objects", () => {
    expect(parseRequiredObject("[]")).toBeNull();
  });

  it("falls back for invalid JSON", () => {
    expect(parseObject("{", { fallback: true })).toEqual({ fallback: true });
  });

  it("clones reactive JSON-compatible values", () => {
    const source = reactive({ definition: { nodes: [{ id: "start" }] } });
    const cloned = cloneJson(source);
    expect(cloned).toEqual({ definition: { nodes: [{ id: "start" }] } });
    expect(cloned).not.toBe(source);
  });
});
