import { describe, expect, it } from "vitest";
import type { FieldLocation } from "../../domain/models";
import { getAtLocation, setAtLocation, cloneTemplate } from "../field-location";

describe("field-location", () => {
  it("reads and writes parameters paths", () => {
    const node = { kind: "mutex", parameters: { name: "lock-a" } };
    const location: FieldLocation = { base: "parameters", path: ["name"] };

    expect(getAtLocation(node, location)).toBe("lock-a");

    const next = setAtLocation(node, location, "lock-b");
    expect(getAtLocation(next, location)).toBe("lock-b");
    expect(getAtLocation(node, location)).toBe("lock-a");
  });

  it("reads and writes wait paths", () => {
    const node = { kind: "wait", wait: { seconds: 60 } };
    const location: FieldLocation = { base: "wait", path: ["seconds"] };

    expect(getAtLocation(node, location)).toBe(60);
    expect(getAtLocation(setAtLocation(node, location, 120), location)).toBe(120);
  });

  it("reads and writes action paths", () => {
    const node = {
      kind: "action",
      action: { provider: "Console", function: "run", configuration: {} },
    };
    const location: FieldLocation = { base: "action", path: ["provider"] };

    expect(getAtLocation(node, location)).toBe("Console");
    expect(getAtLocation(setAtLocation(node, location, "Http"), location)).toBe("Http");
  });

  it("reads and writes transitions paths", () => {
    const node = {
      kind: "condition",
      transitions: { branches: [{ target: { $node: "end" } }], next: { $node: "end" } },
    };
    const location: FieldLocation = { base: "transitions", path: ["branches"] };

    expect(Array.isArray(getAtLocation(node, location))).toBe(true);
    const next = setAtLocation(node, location, []);
    expect(getAtLocation(next, location)).toEqual([]);
  });

  it("reads and writes top-level keys", () => {
    const node = { kind: "loop", max_iterations: 10, parameters: {} };
    const location: FieldLocation = { base: "top_level", path: ["max_iterations"] };

    expect(getAtLocation(node, location)).toBe(10);
    expect(getAtLocation(setAtLocation(node, location, 5), location)).toBe(5);
  });

  it("creates missing intermediate objects when writing", () => {
    const node: { kind: string; wait?: { seconds?: number } } = { kind: "wait" };
    const location: FieldLocation = { base: "wait", path: ["seconds"] };
    const next = setAtLocation(node, location, 30);

    expect(next.wait).toEqual({ seconds: 30 });
    expect(node.wait).toBeUndefined();
  });

  it("clones templates without sharing references", () => {
    const template = { kind: "wait", wait: { seconds: 60 }, parameters: {} };
    const clone = cloneTemplate(template);
    clone.wait = { seconds: 1 };

    expect(template.wait).toEqual({ seconds: 60 });
    expect(clone.wait).toEqual({ seconds: 1 });
  });
});
