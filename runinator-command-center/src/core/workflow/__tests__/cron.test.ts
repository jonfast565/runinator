import { describe, expect, it } from "vitest";
import {
  describeCron,
  expandCronField,
  formatCronRun,
  joinCron,
  matchCronPreset,
  nextCronRuns,
  splitCron,
  validateCron,
  validateCronField,
} from "../cron";

describe("splitCron", () => {
  it("splits a five-field expression", () => {
    expect(splitCron(" 0  9 * * 1-5 ")).toEqual({
      minute: "0",
      hour: "9",
      dayOfMonth: "*",
      month: "*",
      dayOfWeek: "1-5",
    });
  });

  // the six-field (seconds) and @alias forms are valid to croner but have no field builder, so the
  // editor has to be able to tell that it must fall back to raw text rather than mangling them.
  it("declines the forms the builder does not model", () => {
    expect(splitCron("0 0 0 * * *")).toBeNull();
    expect(splitCron("@hourly")).toBeNull();
  });
});

describe("expandCronField", () => {
  it("expands stars, lists, ranges, and steps", () => {
    expect([...(expandCronField("*/15", "minute") ?? [])]).toEqual([0, 15, 30, 45]);
    expect([...(expandCronField("1,3,5", "hour") ?? [])]).toEqual([1, 3, 5]);
    expect([...(expandCronField("9-11", "hour") ?? [])]).toEqual([9, 10, 11]);
    expect([...(expandCronField("10-20/5", "minute") ?? [])]).toEqual([10, 15, 20]);
  });

  it("accepts the three-letter names croner accepts", () => {
    expect([...(expandCronField("MON-FRI", "dayOfWeek") ?? [])]).toEqual([1, 2, 3, 4, 5]);
    expect([...(expandCronField("JAN,DEC", "month") ?? [])]).toEqual([1, 12]);
  });

  // sunday is 0 and 7 in every dialect; treating 7 as out of range would reject a common expression.
  it("folds weekday 7 onto sunday", () => {
    expect([...(expandCronField("7", "dayOfWeek") ?? [])]).toEqual([0]);
  });

  it("rejects out-of-range values, backwards ranges, and junk", () => {
    expect(expandCronField("60", "minute")).toBeNull();
    expect(expandCronField("24", "hour")).toBeNull();
    expect(expandCronField("0", "dayOfMonth")).toBeNull();
    expect(expandCronField("10-2", "hour")).toBeNull();
    expect(expandCronField("*/0", "minute")).toBeNull();
    expect(expandCronField("every", "minute")).toBeNull();
    expect(expandCronField("", "minute")).toBeNull();
  });
});

describe("validateCron", () => {
  it("accepts the presets", () => {
    for (const expression of ["* * * * *", "*/5 * * * *", "0 9 * * 1-5", "0 0 1 * *"]) {
      expect(validateCron(expression)).toBeNull();
    }
  });

  it("names the field at fault", () => {
    expect(validateCronField("99", "minute")).toMatch(/Minute/);
    expect(validateCron("0 99 * * *")).toMatch(/Hour/);
  });

  it("counts the fields it found", () => {
    expect(validateCron("0 0 * *")).toMatch(/found 4/);
    expect(validateCron("   ")).toMatch(/required/);
  });
});

describe("describeCron", () => {
  it("reads common schedules back in english", () => {
    expect(describeCron("* * * * *")).toBe("Every minute (UTC)");
    expect(describeCron("*/5 * * * *")).toBe("Every 5 minutes (UTC)");
    expect(describeCron("0 * * * *")).toBe("Hourly at :00 (UTC)");
    expect(describeCron("30 2 * * *")).toBe("At 02:30 (UTC)");
    expect(describeCron("0 9 * * 1-5")).toBe(
      "At 09:00, on Monday through Friday (UTC)",
    );
    expect(describeCron("0 0 1 * *")).toBe("At 00:00, on day 1 of the month (UTC)");
  });

  it("says nothing about an expression it cannot parse", () => {
    expect(describeCron("@hourly")).toBe("");
    expect(describeCron("0 99 * * *")).toBe("");
  });
});

describe("nextCronRuns", () => {
  const from = new Date(Date.UTC(2026, 7, 17, 12, 30, 15));

  it("projects forward from the next whole minute", () => {
    expect(nextCronRuns("* * * * *", 2, from).map(formatCronRun)).toEqual([
      "2026-08-17 12:31 UTC",
      "2026-08-17 12:32 UTC",
    ]);
  });

  it("honours hour and minute restrictions", () => {
    expect(nextCronRuns("0 9 * * *", 2, from).map(formatCronRun)).toEqual([
      "2026-08-18 09:00 UTC",
      "2026-08-19 09:00 UTC",
    ]);
  });

  // 2026-08-17 is a monday, so a weekday schedule that has already passed today lands on tuesday.
  it("honours weekday restrictions", () => {
    expect(nextCronRuns("0 9 * * 1-5", 1, from).map(formatCronRun)).toEqual([
      "2026-08-18 09:00 UTC",
    ]);
    expect(nextCronRuns("0 9 * * 6,0", 1, from).map(formatCronRun)).toEqual([
      "2026-08-22 09:00 UTC",
    ]);
  });

  it("honours month and day-of-month restrictions", () => {
    expect(nextCronRuns("0 0 1 1 *", 1, from).map(formatCronRun)).toEqual([
      "2027-01-01 00:00 UTC",
    ]);
  });

  // vixie semantics: with both day fields restricted, either one matching is enough.
  it("ors the two day fields when both are restricted", () => {
    const runs = nextCronRuns("0 0 13 * 5", 3, new Date(Date.UTC(2026, 10, 1)));
    expect(runs.map(formatCronRun)).toEqual([
      "2026-11-06 00:00 UTC",
      "2026-11-13 00:00 UTC",
      "2026-11-20 00:00 UTC",
    ]);
  });

  // a schedule that can never match must terminate rather than spin the editor on every keystroke.
  it("gives up on an unsatisfiable date instead of hanging", () => {
    expect(nextCronRuns("0 0 30 2 *", 1, from)).toEqual([]);
  });

  it("returns nothing for an expression it cannot parse", () => {
    expect(nextCronRuns("@hourly", 3, from)).toEqual([]);
  });
});

describe("presets", () => {
  it("recognises a preset expression however it is spaced", () => {
    expect(matchCronPreset("0  9 * *  1-5")).toBe("weekdays");
    expect(matchCronPreset("7 3 * * *")).toBe("custom");
  });

  it("round-trips through the field builder", () => {
    const fields = splitCron("0 9 * * 1-5");
    expect(fields && joinCron(fields)).toBe("0 9 * * 1-5");
  });
});
