import { describe, expect, it } from "vitest";

import {
  buildCursorMarkers,
  coerceRunCursors,
  cursorDebug,
  cursorLabel,
  cursorsByNode,
  isCursorPaused,
  type RunCursor,
} from "../../domain/models/workflow-state";

function cursor(overrides: Partial<RunCursor> & Pick<RunCursor, "id" | "node_id">): RunCursor {
  return { ...overrides };
}

describe("cursorDebug", () => {
  // a run paused by the previous single-cursor debugger carries no per-cursor runtime; it must
  // still read as paused or it silently un-pauses on upgrade.
  it("falls back to the run frame while no cursor carries its own", () => {
    const cursors = [cursor({ id: "a", node_id: "verify" })];

    expect(cursorDebug(cursors, "a", { paused: true })?.paused).toBe(true);
    expect(isCursorPaused(cursors, "a", { paused: true })).toBe(true);
  });

  // this is the rule that keeps the ui honest: once any cursor has been written, the flat frame is
  // the *primary's mirror*, not the run's state. reading it for a sibling would show branches as
  // paused that the reducer will happily keep running.
  it("stops falling back once any cursor carries a runtime", () => {
    const cursors = [
      cursor({ id: "a", node_id: "branch_a", debug: { paused: true } }),
      cursor({ id: "b", node_id: "branch_b" }),
    ];

    expect(isCursorPaused(cursors, "a", { paused: true })).toBe(true);
    expect(isCursorPaused(cursors, "b", { paused: true })).toBe(false);
  });
});

describe("cursorLabel", () => {
  it("names the original thread of control, branches, and forks distinctly", () => {
    expect(cursorLabel(cursor({ id: "a", node_id: "start" }), 0)).toBe("main");
    expect(cursorLabel(cursor({ id: "b", node_id: "x", forked_by: "fork" }), 1)).toBe("fork:x");
    expect(
      cursorLabel(
        cursor({
          id: "c",
          node_id: "x",
          speculative: { forked_from_cursor: "a", label: "what-if-403" },
        }),
        2,
      ),
    ).toBe("what-if-403");
  });
});

describe("buildCursorMarkers", () => {
  it("assigns a stable palette slot per position and flags the selection", () => {
    const cursors = [
      cursor({ id: "a", node_id: "one" }),
      cursor({ id: "b", node_id: "two", debug: { paused: true } }),
    ];

    const markers = buildCursorMarkers(cursors, null, "b");

    expect(markers.map((marker) => marker.paletteIndex)).toEqual([0, 1]);
    expect(markers[1]?.selected).toBe(true);
    expect(markers[1]?.paused).toBe(true);
    expect(markers[0]?.paused).toBe(false);
  });

  // two cursors can share a node -- a fan-out whose branches converge, or a fork walking beside the
  // branch it came from. the node card has to be able to draw both.
  it("groups several cursors standing on one node", () => {
    const markers = buildCursorMarkers(
      [cursor({ id: "a", node_id: "join" }), cursor({ id: "b", node_id: "join" })],
      null,
      null,
    );

    expect(cursorsByNode(markers).get("join")).toHaveLength(2);
  });
});

describe("coerceRunCursors", () => {
  it("parses the wire shape including the speculative frame", () => {
    const cursors = coerceRunCursors([
      {
        id: "a",
        node_id: "call",
        forked_by: "fork",
        speculative: {
          forked_from_cursor: "root",
          label: "what-if",
          armed_nodes: ["call"],
          context_patch: { steps: { fetch: { output: { status: 403 } } } },
        },
        debug: { paused: true },
      },
    ]);

    expect(cursors).toHaveLength(1);
    expect(cursors[0]?.forked_by).toBe("fork");
    expect(cursors[0]?.speculative?.armed_nodes).toEqual(["call"]);
    expect(cursors[0]?.debug?.paused).toBe(true);
  });

  it("drops malformed entries rather than poisoning the list", () => {
    expect(coerceRunCursors([{ id: "a" }, { node_id: "x" }, "nope", null])).toEqual([]);
    expect(coerceRunCursors(undefined)).toEqual([]);
    expect(coerceRunCursors([{ id: "a", node_id: "x" }])).toHaveLength(1);
  });
});
