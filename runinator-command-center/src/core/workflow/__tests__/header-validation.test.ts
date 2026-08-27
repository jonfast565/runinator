import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  declarationIssues,
  headerIssues,
  interruptIssues,
  isRenderableCondition,
} from "../header-validation";
import { setWorkflowCatalogs } from "../catalog-registry";
import { testNodeKindCatalog } from "./catalog-fixtures";
import type { JsonRecord, JsonValue } from "../../domain/json";

/** start -> poll -> end in the main flow; `refresh -> handled` as a wake handler region. */
function definition(overrides: Partial<JsonRecord> = {}): JsonRecord {
  return {
    start: "start",
    nodes: [
      { id: "start", kind: "start", transitions: { next: { $node: "poll" } } },
      { id: "poll", kind: "wait", transitions: { next: { $node: "end" } } },
      { id: "end", kind: "end" },
      { id: "refresh", kind: "audit", transitions: { next: { $node: "handled" } } },
      { id: "handled", kind: "resume", parameters: { mode: "resume" } },
    ],
    metadata: { interrupts: [{ on: "wake", handler: "refresh" }] },
    ...overrides,
  };
}

function messages(definitionValue: JsonRecord): string[] {
  return headerIssues(definitionValue).map((issue) => issue.message);
}

beforeEach(() => {
  setWorkflowCatalogs({ nodeKinds: testNodeKindCatalog, triggerKinds: [], enums: [] });
});

afterEach(() => {
  setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });
});

describe("interrupt declarations", () => {
  it("accepts a well-formed region", () => {
    expect(headerIssues(definition())).toEqual([]);
  });

  it("rejects a source the grammar does not know", () => {
    const broken = definition({ metadata: { interrupts: [{ on: "webhook", handler: "refresh" }] } });

    expect(messages(broken)).toContainEqual(expect.stringContaining("unknown source 'webhook'"));
  });

  it("rejects two handlers for one source", () => {
    const broken = definition({
      metadata: {
        interrupts: [
          { on: "wake", handler: "refresh" },
          { on: "wake", handler: "handled" },
        ],
      },
    });

    expect(messages(broken)).toContainEqual(
      expect.stringContaining("already has a handler; one handler per source"),
    );
  });

  it("rejects a handler that does not exist", () => {
    const broken = definition({ metadata: { interrupts: [{ on: "wake", handler: "ghost" }] } });

    expect(messages(broken)).toContainEqual("Interrupt handler 'ghost' does not exist");
  });

  it("tags interrupt diagnostics with their declared handler", () => {
    const broken = definition({ metadata: { interrupts: [{ on: "wake", handler: "ghost" }] } });

    expect(interruptIssues(broken)).toEqual([
      expect.objectContaining({ interruptHandlerId: "ghost", nodeId: "workflow" }),
    ]);
  });

  it("rejects a handler reachable from the workflow start", () => {
    const broken = definition({ metadata: { interrupts: [{ on: "wake", handler: "poll" }] } });

    expect(messages(broken)).toContainEqual(
      expect.stringContaining("is reachable from the workflow start"),
    );
  });

  it("rejects a region node whose kind is not handler-safe", () => {
    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "refresh", kind: "approval", transitions: { on_success: { $node: "handled" } } },
        { id: "handled", kind: "resume" },
      ],
    });

    expect(messages(broken)).toContainEqual(
      expect.stringContaining("is a approval node, which is not allowed inside a handler region"),
    );
  });

  it("rejects a region that never reaches a resume", () => {
    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "refresh", kind: "audit", transitions: {} },
      ],
      metadata: { interrupts: [{ on: "wake", handler: "refresh" }] },
    });

    expect(messages(broken)).toContainEqual(expect.stringContaining("never reaches a resume node"));
  });

  it("rejects a dangling region member", () => {
    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "refresh", kind: "audit", transitions: { next: { $node: "gone" } } },
      ],
    });

    expect(messages(broken)).toContainEqual(
      "Interrupt handler 'refresh': region node 'gone' does not exist",
    );
  });

  it("rejects a node outside the region transitioning into it", () => {
    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "poll" } } },
        { id: "poll", kind: "wait", transitions: { next: { $node: "end" }, on_failure: { $node: "handled" } } },
        { id: "end", kind: "end" },
        { id: "refresh", kind: "audit", transitions: { next: { $node: "handled" } } },
        { id: "handled", kind: "resume" },
      ],
    });

    expect(messages(broken)).toContainEqual(
      expect.stringContaining("'poll' transitions into the region at 'handled'"),
    );
  });

  it("rejects two regions claiming the same node", () => {
    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "on_wake", kind: "audit", transitions: { next: { $node: "handled" } } },
        { id: "on_child", kind: "audit", transitions: { next: { $node: "handled" } } },
        { id: "handled", kind: "resume" },
      ],
      metadata: {
        interrupts: [
          { on: "wake", handler: "on_wake" },
          { on: "child", handler: "on_child" },
        ],
      },
    });

    expect(messages(broken)).toContainEqual(
      expect.stringContaining("already belongs to handler 'on_wake'"),
    );
  });

  /** the runtime would execute a diamond; the decompiler cannot write one, so warn rather than error. */
  it("warns when a region converges on a node by two paths", () => {
    const converging = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        {
          id: "refresh",
          kind: "condition",
          transitions: {
            branches: [{ when: true, target: { $node: "a" } }],
            next: { $node: "b" },
          },
        },
        { id: "a", kind: "audit", transitions: { next: { $node: "handled" } } },
        { id: "b", kind: "audit", transitions: { next: { $node: "handled" } } },
        { id: "handled", kind: "resume" },
      ],
    });
    const issues = headerIssues(converging);
    const converge = issues.find((issue) => issue.message.includes("more than one path"));

    expect(converge?.severity).toBe("warning");
    expect(converge?.nodeId).toBe("handled");
  });

  /** guessing against an unloaded catalog would flag every region as unsupported. */
  it("skips kind-dependent rules until the catalog loads", () => {
    setWorkflowCatalogs({ nodeKinds: [], triggerKinds: [], enums: [] });

    const broken = definition({
      nodes: [
        { id: "start", kind: "start", transitions: { next: { $node: "end" } } },
        { id: "end", kind: "end" },
        { id: "refresh", kind: "approval", transitions: { on_success: { $node: "handled" } } },
        { id: "handled", kind: "resume" },
      ],
    });

    expect(headerIssues(broken)).toEqual([]);
  });
});

describe("watch guards", () => {
  const watch = (entry: JsonValue) => definition({ metadata: { watches: [entry] } });

  it("accepts a renderable condition pointing at a real node", () => {
    expect(headerIssues(watch({ condition: { value: { $ref: "input.abort" } }, handler: "end" }))).toEqual([]);
  });

  it("rejects a handler that does not exist", () => {
    expect(messages(watch({ condition: { value: true }, handler: "ghost" }))).toContainEqual(
      "Watch guard -> 'ghost': handler node does not exist",
    );
  });

  it("rejects a condition rexrap cannot express", () => {
    expect(messages(watch({ condition: { weird: 1 }, handler: "end" }))).toContainEqual(
      expect.stringContaining("not a shape rexrap can express"),
    );
  });
});

describe("concurrency", () => {
  it("rejects a limit below one, which the decompiler would silently drop", () => {
    const broken = definition({
      metadata: { concurrency: { max_concurrent_runs: 0, on_conflict: "queue" } },
    });

    expect(messages(broken)).toContainEqual(expect.stringContaining("must be at least 1"));
  });

  it("warns that an allow policy never declines", () => {
    const inert = definition({
      metadata: { concurrency: { max_concurrent_runs: 3, on_conflict: "allow" } },
    });

    expect(headerIssues(inert)[0]?.severity).toBe("warning");
  });

  it("accepts an enforced policy", () => {
    const ok = definition({
      metadata: { concurrency: { max_concurrent_runs: 3, on_conflict: "queue" } },
    });

    expect(headerIssues(ok)).toEqual([]);
  });
});

describe("the interrupt / declaration split", () => {
  // the two panels badge themselves from these halves, so a leak either way sends the user to a
  // tab that cannot fix what it is pointing at.
  it("keeps an interrupt problem out of the declaration half", () => {
    const broken = definition({ metadata: { interrupts: [{ on: "wake", handler: "nope" }] } });

    expect(interruptIssues(broken)).not.toEqual([]);
    expect(declarationIssues(broken)).toEqual([]);
  });

  it("keeps a declaration problem out of the interrupt half", () => {
    const broken = definition({
      metadata: { concurrency: { max_concurrent_runs: 0, on_conflict: "skip" } },
    });

    expect(declarationIssues(broken)).not.toEqual([]);
    expect(interruptIssues(broken)).toEqual([]);
  });

  it("still reports both halves together, which is what the canvas table shows", () => {
    const broken = definition({
      metadata: {
        interrupts: [{ on: "wake", handler: "nope" }],
        concurrency: { max_concurrent_runs: 0, on_conflict: "skip" },
      },
    });

    expect(headerIssues(broken)).toEqual([
      ...interruptIssues(broken),
      ...declarationIssues(broken),
    ]);
    expect(headerIssues(broken).length).toBeGreaterThan(1);
  });
});

describe("isRenderableCondition", () => {
  it("accepts every shape the decompiler renders", () => {
    expect(isRenderableCondition({ all: [{ value: true }] })).toBe(true);
    expect(isRenderableCondition({ any: [{ value: { $ref: "a" }, equals: 1 }] })).toBe(true);
    expect(isRenderableCondition({ not: { value: true } })).toBe(true);
    expect(isRenderableCondition({ value: { $ref: "a" }, exists: true })).toBe(true);
    expect(isRenderableCondition({ value: { $ref: "a" }, starts_with: "x" })).toBe(true);
    expect(isRenderableCondition({ value: true })).toBe(true);
  });

  it("rejects everything else", () => {
    expect(isRenderableCondition(true)).toBe(false);
    expect(isRenderableCondition(null)).toBe(false);
    expect(isRenderableCondition([])).toBe(false);
    expect(isRenderableCondition({})).toBe(false);
    expect(isRenderableCondition({ value: 1, unknown_op: 2 })).toBe(false);
    expect(isRenderableCondition({ all: [{ nope: 1 }] })).toBe(false);
  });
});
