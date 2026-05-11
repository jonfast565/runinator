import { describe, expect, it } from "vitest";
import { statusBadgeClass, statusClassForNode } from "../status";

describe("status utils", () => {
  it("maps terminal failures", () => {
    expect(statusBadgeClass("failed")).toBe("status-failed");
    expect(statusClassForNode("timed_out")).toBe("node-danger");
  });

  it("maps active statuses", () => {
    expect(statusBadgeClass("running")).toBe("status-running");
    expect(statusBadgeClass("queued")).toBe("status-waiting");
  });
});
