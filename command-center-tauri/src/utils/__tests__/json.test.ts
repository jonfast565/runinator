import { describe, expect, it } from "vitest";
import { parseObject, parseRequiredObject } from "../json";

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
});
