import { describe, expect, it } from "vitest";
import { genericRecordSummary, genericRecordType } from "../resources";

describe("resource utils", () => {
  it("derives jira summaries from key and title", () => {
    expect(genericRecordSummary({ provider: "jira", key: "ABC-1", title: "Fix it" })).toBe(
      "ABC-1 Fix it",
    );
  });

  it("uses explicit type fields first", () => {
    expect(genericRecordType({ approval_type: "manual" }, "approvals")).toBe("manual");
  });

  it("falls back to endpoint type names", () => {
    expect(genericRecordType({}, "external_items")).toBe("external_item");
    expect(genericRecordType({}, "automation_events")).toBe("event");
  });
});
