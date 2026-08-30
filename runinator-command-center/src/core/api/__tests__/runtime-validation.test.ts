import { describe, expect, it } from "vitest";
import { assertTransportSafe } from "../runtime";

describe("command input validation", () => {
  it("accepts nested JSON and binary values", () => {
    expect(() => {
      assertTransportSafe({ request: { name: "run", count: 2, bytes: new Uint8Array([1, 2]) } });
    }).not.toThrow();
  });

  it("rejects non-finite numbers with their field path", () => {
    expect(() => {
      assertTransportSafe({ request: { desired: Number.NaN } });
    }).toThrow("value.request.desired must be a finite number");
  });

  it("rejects circular request payloads", () => {
    const request: Record<string, unknown> = {};
    request.self = request;
    expect(() => {
      assertTransportSafe(request);
    }).toThrow("value.self contains a circular reference");
  });
});
