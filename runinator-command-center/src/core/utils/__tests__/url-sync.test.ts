import { describe, expect, it } from "vitest";
import { formatRoute, parseRoute } from "../url-sync";

const known = (tab: string) => ["Workflows", "Runs", "Providers"].includes(tab);

describe("url-sync route mapping", () => {
  it("parses an empty hash to no route", () => {
    expect(parseRoute("", known)).toEqual({ tab: null, id: null });
    expect(parseRoute("#", known)).toEqual({ tab: null, id: null });
  });

  it("parses a tab-only hash", () => {
    expect(parseRoute("#/Runs", known)).toEqual({ tab: "Runs", id: null });
    expect(parseRoute("#Runs", known)).toEqual({ tab: "Runs", id: null });
  });

  it("parses a tab + id hash and decodes the id", () => {
    expect(parseRoute("#/Workflows/abc-123", known)).toEqual({ tab: "Workflows", id: "abc-123" });
    expect(parseRoute("#/Runs/a%2Fb", known)).toEqual({ tab: "Runs", id: "a/b" });
  });

  it("rejects unknown tabs", () => {
    expect(parseRoute("#/Nope/1", known)).toEqual({ tab: null, id: "1" });
  });

  it("formats routes with and without an id", () => {
    expect(formatRoute("Runs", null)).toBe("#/Runs");
    expect(formatRoute("Workflows", "abc-123")).toBe("#/Workflows/abc-123");
    expect(formatRoute("Runs", "a/b")).toBe("#/Runs/a%2Fb");
  });

  it("round-trips a tab + id", () => {
    const hash = formatRoute("Workflows", "wf 1");
    expect(parseRoute(hash, known)).toEqual({ tab: "Workflows", id: "wf 1" });
  });
});
