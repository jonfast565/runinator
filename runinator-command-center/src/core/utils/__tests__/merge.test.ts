import { describe, expect, it } from "vitest";
import { mergeById } from "../merge";

interface Row {
  id: string;
  status: string;
}

describe("mergeById", () => {
  it("preserves object identity for unchanged rows and patches them in place", () => {
    const a = { id: "a", status: "running" };
    const b = { id: "b", status: "queued" };
    const merged = mergeById([a, b], [
      { id: "a", status: "succeeded" },
      { id: "b", status: "queued" },
    ]);

    expect(merged[0]).toBe(a);
    expect(merged[0].status).toBe("succeeded");
    expect(merged[1]).toBe(b);
  });

  it("follows incoming order and drops rows absent from incoming", () => {
    const a = { id: "a", status: "x" };
    const b = { id: "b", status: "y" };
    const merged = mergeById([a, b], [{ id: "b", status: "y" }]);

    expect(merged.map((row) => row.id)).toEqual(["b"]);
    expect(merged[0]).toBe(b);
  });

  it("adds new rows using the incoming object", () => {
    const a = { id: "a", status: "x" };
    const c = { id: "c", status: "z" };
    const merged = mergeById([a], [a, c]);

    expect(merged[1]).toBe(c);
    expect(merged.map((row) => row.id)).toEqual(["a", "c"]);
  });

  it("supports a custom key accessor", () => {
    const rows = [{ ref: "1" }, { ref: "2" }];
    const merged = mergeById<{ ref: string }>(rows, [{ ref: "2" }], (row) => row.ref);

    expect(merged[0]).toBe(rows[1]);
  });
});
