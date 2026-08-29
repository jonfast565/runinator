import { describe, expect, it } from "vitest";
import {
  actionMetaRows,
  actionParameterSettings,
  compensationRows,
  nodeSettingRows,
  retrySummary,
} from "../step-detail-format";

function row(rows: { label: string; value: string }[], label: string): string | undefined {
  return rows.find((entry) => entry.label === label)?.value;
}

describe("actionMetaRows", () => {
  it("keeps the executor's call deadline separate from node-owned settings", () => {
    const rows = actionMetaRows(
      { kind: "action", action: { timeout_seconds: 60 }, timeout_seconds: 900 },
      "webhook",
      "send",
    );
    expect(row(rows, "Call Timeout")).toBe("60s");
    expect(row(rows, "Node Timeout")).toBeUndefined();
    expect(row(rows, "Retry")).toBeUndefined();
  });

  it("distinguishes an absent node deadline from a defaulted call deadline", () => {
    const rows = actionMetaRows({ kind: "action", action: {} }, "webhook", "send");
    expect(row(rows, "Call Timeout")).toBe("default");
  });

  it("falls back to a dash for an unconfigured action", () => {
    const rows = actionMetaRows({ kind: "action" }, "", "");
    expect(row(rows, "Provider")).toBe("—");
    expect(row(rows, "Function")).toBe("—");
  });
});

describe("nodeSettingRows", () => {
  it("shows runtime switches and policy values owned by the node", () => {
    const rows = nodeSettingRows({
      skipped: true,
      locked: true,
      timeout_seconds: 900,
      retry: { max_attempts: 1 },
    });
    expect(row(rows, "State")).toBe("Skipped");
    expect(row(rows, "Locked")).toBe("Yes");
    expect(row(rows, "Node Timeout")).toBe("900s");
    expect(row(rows, "Retry")).toMatch(/Runs once/);
  });
});

describe("actionParameterSettings", () => {
  const parameters = [
    {
      name: "url",
      ty: { type: "string" as const },
      required: true,
      secret: false,
    },
    {
      name: "method",
      ty: { type: "string" as const },
      required: false,
      default_value: "POST",
      secret: false,
    },
    {
      name: "comment",
      ty: { type: "string" as const },
      required: false,
      secret: false,
      description: "Documentation that does not belong in the settings view.",
    },
  ];

  it("shows configured values and effective defaults, but omits unset schema documentation", () => {
    expect(actionParameterSettings(parameters, { url: "https://example.test" })).toEqual([
      {
        name: "url",
        type: "string",
        value: "https://example.test",
        source: "configured",
        secret: false,
      },
      {
        name: "method",
        type: "string",
        value: "POST",
        source: "default",
        secret: false,
      },
    ]);
  });

  it("falls back to raw configured keys when provider metadata is unavailable", () => {
    expect(actionParameterSettings([], { body: { ok: true } })).toEqual([
      {
        name: "body",
        type: "",
        value: "ok: true",
        source: "configured",
        secret: false,
      },
    ]);
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
