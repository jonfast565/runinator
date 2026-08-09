import { describe, expect, it } from "vitest";

import {
  FALLBACK_NODE_WIDTH,
  TOKEN_LANE_GAP,
  TOKEN_LIFT,
  advanceHopSequence,
  buildCursorTokens,
  type NodeBox,
} from "../cursor-motion";
import type { CursorMarker } from "../../domain/models/workflow-state";

function marker(overrides: Partial<CursorMarker> & Pick<CursorMarker, "id" | "nodeId">) {
  return {
    paletteIndex: 0,
    label: overrides.id,
    paused: false,
    speculative: false,
    armed: false,
    selected: false,
    ...overrides,
  } satisfies CursorMarker;
}

function boxes(entries: Record<string, NodeBox>): Map<string, NodeBox> {
  return new Map(Object.entries(entries));
}

describe("buildCursorTokens", () => {
  it("rests a token centred above its node's top edge", () => {
    const [token] = buildCursorTokens(
      [marker({ id: "a", nodeId: "verify" })],
      boxes({ verify: { x: 100, y: 200, width: 180, height: 60 } }),
    );

    expect(token?.x).toBe(190);
    expect(token?.y).toBe(200 - TOKEN_LIFT);
  });

  // two branches that converged on one node have to stay tellable apart, which is the whole reason
  // the tokens fan out instead of stacking exactly on top of each other.
  it("fans cursors sharing a node out symmetrically about the centre", () => {
    const [left, right] = buildCursorTokens(
      [marker({ id: "a", nodeId: "join" }), marker({ id: "b", nodeId: "join" })],
      boxes({ join: { x: 0, y: 0, width: 200, height: 60 } }),
    );

    expect(left?.x).toBe(100 - TOKEN_LANE_GAP / 2);
    expect(right?.x).toBe(100 + TOKEN_LANE_GAP / 2);
    // symmetric about the node's centre, so the pair still reads as sitting on this node.
    expect((left!.x + right!.x) / 2).toBe(100);
  });

  // a subflow's child run walks nodes this definition does not contain. a token pinned at the
  // origin would read as a real position on an unrelated node.
  it("drops a cursor standing on a node the graph does not hold", () => {
    const tokens = buildCursorTokens(
      [marker({ id: "a", nodeId: "elsewhere" }), marker({ id: "b", nodeId: "here" })],
      boxes({ here: { x: 10, y: 10, width: 100, height: 40 } }),
    );

    expect(tokens.map((token) => token.id)).toEqual(["b"]);
  });

  it("centres on an assumed width until the renderer has measured the node", () => {
    const [token] = buildCursorTokens(
      [marker({ id: "a", nodeId: "fresh" })],
      boxes({ fresh: { x: 0, y: 0, width: 0, height: 0 } }),
    );

    expect(token?.x).toBe(FALLBACK_NODE_WIDTH / 2);
  });

  it("carries the marker's identity through so the token draws like its rail row", () => {
    const [token] = buildCursorTokens(
      [marker({ id: "a", nodeId: "n", paletteIndex: 3, speculative: true, paused: true })],
      boxes({ n: { x: 0, y: 0, width: 10, height: 10 } }),
    );

    expect(token).toMatchObject({ paletteIndex: 3, speculative: true, paused: true });
  });
});

describe("advanceHopSequence", () => {
  function token(id: string, nodeId: string) {
    return { ...marker({ id, nodeId }), x: 0, y: 0 };
  }

  // the parity of the counter is what picks between two identical keyframe sets, and flipping it
  // is the only thing that replays the jump. a cursor that stayed put must not flip.
  it("bumps only the cursors that changed node", () => {
    const previous = new Map([
      ["a", "one"],
      ["b", "two"],
    ]);
    const { sequence, positions } = advanceHopSequence(
      [token("a", "three"), token("b", "two")],
      previous,
      new Map([
        ["a", 4],
        ["b", 7],
      ]),
    );

    expect(sequence.get("a")).toBe(5);
    expect(sequence.get("b")).toBe(7);
    expect(positions).toEqual(
      new Map([
        ["a", "three"],
        ["b", "two"],
      ]),
    );
  });

  // a fresh fork has nowhere to have jumped from; flying it in from a node it was never on would
  // draw a path the run never took.
  it("does not bump a cursor it is seeing for the first time", () => {
    const { sequence } = advanceHopSequence([token("new", "start")], new Map(), new Map());

    expect(sequence.get("new")).toBe(0);
  });

  it("forgets a retired cursor rather than carrying its counter forever", () => {
    const { sequence, positions } = advanceHopSequence(
      [token("a", "one")],
      new Map([
        ["a", "one"],
        ["gone", "two"],
      ]),
      new Map([
        ["a", 1],
        ["gone", 9],
      ]),
    );

    expect(sequence.has("gone")).toBe(false);
    expect(positions.has("gone")).toBe(false);
  });
});
