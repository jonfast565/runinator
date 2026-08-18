import { describe, expect, it } from "vitest";
import {
  describeRetryPolicy,
  formatDuration,
  retryDelays,
  retryWindowSeconds,
  type RetryPolicy,
} from "../retry";

function policy(overrides: Partial<RetryPolicy> = {}): RetryPolicy {
  return {
    max_attempts: 1,
    backoff_base_seconds: 1,
    backoff_max_seconds: 300,
    jitter: false,
    retry_on: "any",
    ...overrides,
  };
}

describe("retryDelays", () => {
  // the schedule has to agree with `retry_backoff_delay` in the reducer: the delay before attempt
  // n+1 is computed from attempt n, so N attempts produce N-1 delays starting at `base * 2^0`.
  it("doubles from the base, one delay short of the attempt count", () => {
    expect(retryDelays(policy({ max_attempts: 4, backoff_base_seconds: 2 }))).toEqual([2, 4, 8]);
  });

  it("caps at the maximum", () => {
    expect(
      retryDelays(policy({ max_attempts: 6, backoff_base_seconds: 10, backoff_max_seconds: 30 })),
    ).toEqual([10, 20, 30, 30, 30]);
  });

  it("has no delays when the node never retries", () => {
    expect(retryDelays(policy({ max_attempts: 1 }))).toEqual([]);
    expect(retryDelays(policy({ max_attempts: 0 }))).toEqual([]);
  });

  // the reducer clamps `max` up to `base` rather than letting an inverted pair produce nothing.
  it("treats a maximum below the base as the base", () => {
    expect(
      retryDelays(policy({ max_attempts: 3, backoff_base_seconds: 60, backoff_max_seconds: 5 })),
    ).toEqual([60, 60]);
  });

  it("does not run away on a large attempt count", () => {
    const delays = retryDelays(
      policy({ max_attempts: 40, backoff_base_seconds: 1, backoff_max_seconds: 3600 }),
    );
    expect(delays).toHaveLength(39);
    expect(delays.every((delay) => Number.isFinite(delay) && delay <= 3600)).toBe(true);
  });
});

describe("formatDuration", () => {
  it("picks the shortest readable unit", () => {
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(60)).toBe("1m");
    expect(formatDuration(150)).toBe("2m 30s");
    expect(formatDuration(3600)).toBe("1h");
    expect(formatDuration(3900)).toBe("1h 5m");
  });
});

describe("describeRetryPolicy", () => {
  it("says so when the node never retries", () => {
    expect(describeRetryPolicy(policy())).toMatch(/Runs once/);
  });

  it("reads the schedule back", () => {
    expect(
      describeRetryPolicy(policy({ max_attempts: 3, backoff_base_seconds: 2, retry_on: "timeout" })),
    ).toBe("Up to 2 retries on timeout only, waiting 2s, then 4s.");
  });

  it("mentions jitter", () => {
    expect(describeRetryPolicy(policy({ max_attempts: 2, jitter: true }))).toMatch(
      /randomized down to half/,
    );
  });
});

describe("retryWindowSeconds", () => {
  it("totals the delays", () => {
    expect(retryWindowSeconds(policy({ max_attempts: 4, backoff_base_seconds: 2 }))).toBe(14);
    expect(retryWindowSeconds(policy())).toBe(0);
  });
});
