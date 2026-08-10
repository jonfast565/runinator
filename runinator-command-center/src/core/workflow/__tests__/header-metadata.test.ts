import { describe, expect, it } from "vitest";

import {
  applyWorkflowHeader,
  emptyWorkflowHeader,
  isEmptyWorkflowHeader,
  readWorkflowHeader,
} from "../header-metadata";
import type { JsonRecord } from "../../domain/json";

/** a definition carrying all four header keys plus two this module must not touch. */
function definition(): JsonRecord {
  return {
    start: "start",
    nodes: [{ id: "start", kind: "start" }],
    metadata: {
      wdl: { types: [] },
      triggers: [{ kind: "cron", configuration: { cron: "0 * * * *" } }],
      interrupts: [{ on: "wake", handler: "refresh" }],
      watches: [{ condition: { value: { $ref: "input.abort" } }, handler: "cleanup" }],
      concurrency: { max_concurrent_runs: 2, on_conflict: "queue" },
      correlation: { $ref: "input.batch_id" },
    },
  };
}

describe("readWorkflowHeader", () => {
  it("reads all four declarations, renaming `on` to `source`", () => {
    const header = readWorkflowHeader(definition());

    expect(header.interrupts).toEqual([{ source: "wake", handler: "refresh", enabled: true }]);
    expect(header.watches).toEqual([
      { condition: { value: { $ref: "input.abort" } }, handler: "cleanup" },
    ]);
    expect(header.concurrency).toEqual({ maxConcurrentRuns: 2, onConflict: "queue" });
    expect(header.correlation).toEqual({ $ref: "input.batch_id" });
  });

  it("keeps metadata authoritative over source-neutral graph entries", () => {
    const header = readWorkflowHeader({
      nodes: [{ id: "on_timeout", kind: "interrupt", parameters: { on: "timeout" } }],
      metadata: { interrupts: [{ on: "wake", handler: "stale" }] },
    });

    expect(header.interrupts).toEqual([{ source: "wake", handler: "stale", enabled: true }]);
  });

  it("is empty for a definition with no metadata", () => {
    expect(readWorkflowHeader({ nodes: [] })).toEqual(emptyWorkflowHeader());
    expect(isEmptyWorkflowHeader(readWorkflowHeader({ nodes: [] }))).toBe(true);
  });

  it("drops half-written entries the backend would refuse to decode", () => {
    const header = readWorkflowHeader({
      metadata: {
        interrupts: [{ on: "wake" }, { handler: "refresh" }, { on: "child", handler: "notify" }],
        watches: [{ condition: true }],
      },
    });

    expect(header.interrupts).toEqual([{ source: "child", handler: "notify", enabled: true }]);
    expect(header.watches).toEqual([]);
  });

  it("defaults a concurrency header's missing halves rather than dropping it", () => {
    expect(readWorkflowHeader({ metadata: { concurrency: {} } })).toMatchObject({
      concurrency: { maxConcurrentRuns: 0, onConflict: "allow" },
    });
  });

  /** `correlation` is a bare expression, so a falsy literal is still a declared key. */
  it("keeps a falsy correlation expression", () => {
    expect(readWorkflowHeader({ metadata: { correlation: 0 } }).correlation).toBe(0);
    expect(readWorkflowHeader({ metadata: {} }).correlation).toBeNull();
  });
});

describe("applyWorkflowHeader", () => {
  it("round-trips a header through a definition unchanged", () => {
    const target = definition();
    const header = readWorkflowHeader(target);

    applyWorkflowHeader(target, header);

    expect(readWorkflowHeader(target)).toEqual(header);
    expect(target).toEqual(definition());
  });

  it("deletes a section's key rather than writing an empty one", () => {
    const target = definition();

    applyWorkflowHeader(target, emptyWorkflowHeader());

    const metadata = target.metadata as JsonRecord;
    expect(Object.keys(metadata).sort()).toEqual(["triggers", "wdl"]);
  });

  it("leaves metadata keys it does not own alone", () => {
    const target = definition();

    applyWorkflowHeader(target, { ...emptyWorkflowHeader(), correlation: "batch-1" });

    const metadata = target.metadata as JsonRecord;
    expect(metadata.wdl).toEqual({ types: [] });
    expect(metadata.triggers).toHaveLength(1);
    expect(metadata.correlation).toBe("batch-1");
  });

  it("does not create an empty metadata object on an untouched definition", () => {
    const target: JsonRecord = { start: "start", nodes: [] };

    applyWorkflowHeader(target, emptyWorkflowHeader());

    expect("metadata" in target).toBe(false);
  });

  it("writes interrupts back under the wire name `on`", () => {
    const target: JsonRecord = { nodes: [] };

    applyWorkflowHeader(target, {
      ...emptyWorkflowHeader(),
      interrupts: [{ source: "external", handler: "on_external", enabled: true }],
      concurrency: { maxConcurrentRuns: 4, onConflict: "skip" },
    });

    expect(target.metadata).toEqual({
      interrupts: [{ on: "external", handler: "on_external" }],
      concurrency: { max_concurrent_runs: 4, on_conflict: "skip" },
    });
  });

  it("writes disabled explicitly and omits the enabled default", () => {
    const target: JsonRecord = { nodes: [] };

    applyWorkflowHeader(target, {
      ...emptyWorkflowHeader(),
      interrupts: [
        { source: "wake", handler: "on_wake", enabled: false },
        { source: "timeout", handler: "on_timeout", enabled: true },
      ],
    });

    expect((target.metadata as JsonRecord).interrupts).toEqual([
      { on: "wake", handler: "on_wake", enabled: false },
      { on: "timeout", handler: "on_timeout" },
    ]);
  });
});
