import { describe, expect, it } from "vitest";
import { describeBulkResult, runBulk } from "../bulk";

describe("runBulk", () => {
  it("collects successes without rejecting", async () => {
    const result = await runBulk([1, 2, 3], () => Promise.resolve());
    expect(result.succeeded).toEqual([1, 2, 3]);
    expect(result.failed).toEqual([]);
    expect(result.allFailed).toBe(false);
  });

  it("keeps going past a failure and reports the failed items", async () => {
    const result = await runBulk([1, 2, 3, 4], (item) =>
      item % 2 === 0 ? Promise.reject(new Error(`no ${String(item)}`)) : Promise.resolve(),
    );

    expect(result.succeeded.sort()).toEqual([1, 3]);
    expect(result.failed.map((failure) => failure.item).sort()).toEqual([2, 4]);
    expect(result.failed[0]?.message).toContain("no ");
    expect(result.allFailed).toBe(false);
  });

  it("flags an all-failed batch", async () => {
    const result = await runBulk([1, 2], () => Promise.reject(new Error("down")));
    expect(result.allFailed).toBe(true);
    expect(result.succeeded).toEqual([]);
  });

  it("treats an empty batch as not all-failed", async () => {
    const result = await runBulk([], () => Promise.resolve());
    expect(result.allFailed).toBe(false);
  });

  it("never exceeds the concurrency limit", async () => {
    let inFlight = 0;
    let peak = 0;
    const result = await runBulk(
      Array.from({ length: 12 }, (_, index) => index),
      async () => {
        inFlight += 1;
        peak = Math.max(peak, inFlight);
        await Promise.resolve();
        inFlight -= 1;
      },
      { concurrency: 3 },
    );

    expect(peak).toBeLessThanOrEqual(3);
    expect(result.succeeded).toHaveLength(12);
  });

  it("stops starting work once the signal aborts", async () => {
    const controller = new AbortController();
    let started = 0;
    const result = await runBulk(
      Array.from({ length: 10 }, (_, index) => index),
      async () => {
        started += 1;

        if (started === 2) {
          controller.abort();
        }

        await Promise.resolve();
      },
      { concurrency: 1, signal: controller.signal },
    );

    expect(started).toBe(2);
    expect(result.succeeded).toHaveLength(2);
  });

  it("stringifies non-Error rejections", async () => {
    const result = await runBulk([1], () => Promise.reject(new Error("plain string")));
    expect(result.failed[0]?.message).toBe("plain string");
  });
});

describe("describeBulkResult", () => {
  it("describes a clean batch", () => {
    expect(
      describeBulkResult({ succeeded: [1, 2], failed: [], allFailed: false }, "Disabled", "workflow"),
    ).toBe("Disabled 2 workflows");
  });

  it("singularizes a batch of one", () => {
    expect(
      describeBulkResult({ succeeded: [1], failed: [], allFailed: false }, "Canceled", "run"),
    ).toBe("Canceled 1 run");
  });

  it("describes a partial failure", () => {
    const text = describeBulkResult(
      {
        succeeded: [1],
        failed: [{ item: 2, error: new Error("boom"), message: "boom" }],
        allFailed: false,
      },
      "Deleted",
      "workflow",
    );
    expect(text).toBe("Deleted 1 of 2 workflows (1 failed: boom)");
  });

  it("describes a total failure in the same shape", () => {
    const text = describeBulkResult(
      {
        succeeded: [],
        failed: [{ item: 1, error: new Error("boom"), message: "boom" }],
        allFailed: true,
      },
      "Deleted",
      "workflow",
    );
    expect(text).toBe("Deleted 0 of 1 workflow (1 failed: boom)");
  });
});
