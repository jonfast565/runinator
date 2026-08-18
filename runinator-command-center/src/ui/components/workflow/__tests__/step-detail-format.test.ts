import { describe, expect, it } from "vitest";
import { actionMetaRows, compensationRows, retrySummary } from "../step-detail-format";

function row(rows: { label: string; value: string }[], label: string): string | undefined {
  return rows.find((entry) => entry.label === label)?.value;
}

describe("actionMetaRows", () => {
  // the two deadlines are different fields with different owners — the worker's call deadline and
  // the reducer's node deadline — and showing one number for both is what let an edit land on the
  // field nobody was looking at.
  it("lists the call and node deadlines separately", () => {
    const rows = actionMetaRows(
      { kind: "action", action: { timeout_seconds: 60 }, timeout_seconds: 900 },
      "webhook",
      "send",
    );
    expect(row(rows, "Call Timeout")).toBe("60s");
    expect(row(rows, "Node Timeout")).toBe("900s");
  });

  it("distinguishes an absent node deadline from a defaulted call deadline", () => {
    const rows = actionMetaRows({ kind: "action", action: {} }, "webhook", "send");
    expect(row(rows, "Call Timeout")).toBe("default");
    expect(row(rows, "Node Timeout")).toBe("none");
  });

  it("falls back to a dash for an unconfigured action", () => {
    const rows = actionMetaRows({ kind: "action" }, "", "");
    expect(row(rows, "Provider")).toBe("—");
    expect(row(rows, "Function")).toBe("—");
  });
});

describe("retrySummary", () => {
  it("reads the whole policy, not just the attempt count", () => {
    expect(
      retrySummary({
        retry: {
          max_attempts: 3,
          backoff_base_seconds: 5,
          backoff_max_seconds: 300,
          jitter: true,
          retry_on: "failure",
        },
      }),
    ).toBe("Up to 2 retries on failure only, waiting 5s, then 10s, each randomized down to half.");
  });

  it("defaults a node that declares no retry to a single attempt", () => {
    expect(retrySummary({})).toMatch(/Runs once/);
  });
});

describe("compensationRows", () => {
  it("is empty for a node that declares none", () => {
    expect(compensationRows({ kind: "action" })).toEqual([]);
  });

  it("names the call and the parameters it carries", () => {
    const rows = compensationRows({
      compensation: {
        provider: "webhook",
        function: "send",
        timeout_seconds: 300,
        configuration: { url: "rollback", body: {} },
      },
    });
    expect(row(rows, "Provider")).toBe("webhook");
    expect(row(rows, "Function")).toBe("send");
    expect(row(rows, "Timeout")).toBe("300s");
    expect(row(rows, "Parameters")).toBe("url, body");
  });

  it("says so when the compensation takes no parameters", () => {
    const rows = compensationRows({
      compensation: { provider: "webhook", function: "send", configuration: {} },
    });
    expect(row(rows, "Parameters")).toBe("none");
    expect(row(rows, "Timeout")).toBe("default");
  });
});
