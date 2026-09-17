import { describe, expect, it } from "vitest";
import { formatRelativeTime } from "../time";

const NOW = Date.parse("2026-09-17T12:00:00Z");

describe("formatRelativeTime", () => {
  it("uses compact minute, hour, and day units", () => {
    expect(formatRelativeTime("2026-09-17T11:58:30Z", { now: NOW })).toBe("1m ago");
    expect(formatRelativeTime("2026-09-17T09:00:00Z", { now: NOW })).toBe("3h ago");
    expect(formatRelativeTime("2026-09-15T12:00:00Z", { now: NOW })).toBe("2d ago");
  });

  it("supports surface-specific labels without duplicating time arithmetic", () => {
    expect(
      formatRelativeTime("2026-09-17T11:59:45Z", {
        now: NOW,
        prefix: "Updated ",
        nowLabel: "Updated now",
      }),
    ).toBe("Updated now");
    expect(formatRelativeTime("2026-09-17T11:55:00Z", { now: NOW, prefix: "Updated " })).toBe(
      "Updated 5m ago",
    );
  });

  it("preserves invalid values and clamps future timestamps to now", () => {
    expect(formatRelativeTime("not-a-date", { now: NOW })).toBe("not-a-date");
    expect(formatRelativeTime("2026-09-18T12:00:00Z", { now: NOW, nowLabel: "Just now" })).toBe(
      "Just now",
    );
  });
});
