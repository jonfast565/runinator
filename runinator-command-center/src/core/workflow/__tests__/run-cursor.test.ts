import { describe, expect, it } from "vitest";

import {
  buildCursorMarkers,
  buildTerminalCursorMarker,
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

  // This keeps the UI honest: once any cursor is written, the flat frame is
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

  // arming is per node: the rail's toggle must reflect the branch's *current* position, not that it
  // armed some other node earlier. reporting a stale arm would tell the operator a shadowed node is
  // about to dispatch for real, or the reverse.
  it("flags a speculative branch as armed only on the node it is standing on", () => {
    const [armed, elsewhere, real] = buildCursorMarkers(
      [
        cursor({
          id: "a",
          node_id: "charge",
          speculative: { forked_from_cursor: "root", armed_nodes: ["charge"] },
        }),
        cursor({
          id: "b",
          node_id: "notify",
          speculative: { forked_from_cursor: "root", armed_nodes: ["charge"] },
        }),
        cursor({ id: "c", node_id: "charge" }),
      ],
      null,
      null,
    );

    expect(armed.armed).toBe(true);
    expect(elsewhere.armed).toBe(false);
    // a real cursor never shadows, so "armed" is not a state it can be in.
    expect(real.armed).toBe(false);
  });
});

describe("buildTerminalCursorMarker", () => {
  it("moves the selected live cursor to its completed endpoint", () => {
    const marker = buildTerminalCursorMarker(
      [cursor({ id: "a", node_id: "send" }), cursor({ id: "b", node_id: "wait" })],
      "end",
      "run-1",
      null,
      "b",
    );

    expect(marker).toMatchObject({
      id: "b",
      nodeId: "end",
      label: "wait",
      paused: true,
      terminal: true,
      selected: true,
    });
  });

  it("still draws a completed main marker when the runtime has already cleared cursors", () => {
    expect(buildTerminalCursorMarker([], "end", "run-1")).toMatchObject({
      id: "terminal:run-1",
      nodeId: "end",
      label: "main",
      terminal: true,
    });
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

describe("interrupt cursors", () => {
  const wireCursors = [
    { id: "main", node_id: "poll", suspended_by: "handler", suspended_seconds: 42 },
    {
      id: "handler",
      node_id: "refresh",
      interrupt: {
        interrupted_cursor: "main",
        source: "wake",
        payload: { deadline_unix: 42 },
        resume: { node_id: "poll" },
      },
    },
  ];

  it("parses the interrupt frame and the suspension off the wire", () => {
    const cursors = coerceRunCursors(wireCursors);

    expect(cursors[0]?.suspended_by).toBe("handler");
    expect(cursors[0]?.suspended_seconds).toBe(42);
    expect(cursors[1]?.interrupt?.source).toBe("wake");
    expect(cursors[1]?.interrupt?.interrupted_cursor).toBe("main");
    expect(cursors[1]?.interrupt?.resume?.node_id).toBe("poll");
  });

  it("names a handler for what raised it rather than where it stands", () => {
    const cursors = coerceRunCursors(wireCursors);
    const markers = buildCursorMarkers(cursors);

    expect(markers[1]?.label).toBe("wake handler");
    expect(markers[1]?.interruptSource).toBe("wake");
    expect(markers[0]?.suspended).toBe(true);
    expect(markers[1]?.suspended).toBe(false);
  });

  it("leaves an ordinary thread unmarked", () => {
    const markers = buildCursorMarkers(coerceRunCursors([{ id: "a", node_id: "call" }]));

    expect(markers[0]?.interruptSource).toBeNull();
    expect(markers[0]?.suspended).toBe(false);
    expect(markers[0]?.label).toBe("main");
  });
});
