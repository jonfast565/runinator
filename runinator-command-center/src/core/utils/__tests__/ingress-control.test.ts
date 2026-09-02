import { describe, expect, it } from "vitest";
import type { JsonRecord } from "../../domain/models";
import { ingressLane, recordsForIngressLane } from "../ingress-control";

function record(id: number, state: string): JsonRecord {
  return {
    id: String(id),
    state,
    received_at: "2026-01-01T00:00:00Z",
    resolved_at: "2026-01-01T00:00:01Z",
  };
}

describe("ingress-control lanes", () => {
  it("keeps resolved messages visible after their dwell window", () => {
    const applied = record(1, "applied");
    const dropped = record(2, "dropped");

    expect(recordsForIngressLane([applied, dropped], "applied")).toEqual([applied]);
    expect(recordsForIngressLane([applied, dropped], "dropped")).toEqual([dropped]);
  });

  it("groups in-flight and failed application states in the applied lane", () => {
    expect(ingressLane(record(1, "approved"))).toBe("applied");
    expect(ingressLane(record(2, "applying"))).toBe("applied");
    expect(ingressLane(record(3, "failed"))).toBe("applied");
  });

  it("bounds completed history while retaining all held messages", () => {
    const applied = Array.from({ length: 60 }, (_, index) => record(index, "applied"));
    const held = Array.from({ length: 60 }, (_, index) => record(index, "held"));

    expect(recordsForIngressLane(applied, "applied")).toHaveLength(50);
    expect(recordsForIngressLane(held, "held")).toHaveLength(60);
  });
});
