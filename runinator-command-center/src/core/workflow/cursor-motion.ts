import type { CursorMarker } from "../domain/models/workflow-state";

/** a node's box in flow coordinates, as measured by the renderer. */
export interface NodeBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** a thread of control drawn as a token travelling above the graph. */
export interface CursorToken extends CursorMarker {
  /** where the token rests, in flow coordinates: centred above its node's top edge. */
  x: number;
  y: number;
}

/**
 * width assumed for a node the renderer has not measured yet. only ever visible for the frame or
 * two before layout settles; a wrong guess shifts a token horizontally, it does not misplace it.
 */
export const FALLBACK_NODE_WIDTH = 180;

/** how far above a node's top edge a resting token sits. */
export const TOKEN_LIFT = 14;

/** horizontal spacing between tokens sharing one node. */
export const TOKEN_LANE_GAP = 13;

/**
 * place every cursor as a token in flow coordinates.
 *
 * a token rests centred above the node its cursor stands on, so the resting picture matches what
 * the node cards drew before; the motion between two placements is what the renderer animates.
 * cursors sharing a node fan out symmetrically about the centre rather than stacking, which is the
 * only way two branches that converged stay tellable apart.
 *
 * a cursor whose node is not in the graph is dropped: a subflow's child run walks nodes this
 * definition does not contain, and a token pinned at the origin would read as a real position.
 */
export function buildCursorTokens(
  markers: CursorMarker[],
  boxes: Map<string, NodeBox>,
): CursorToken[] {
  const lanes = new Map<string, number>();
  const crowd = new Map<string, number>();

  for (const marker of markers) {
    if (boxes.has(marker.nodeId)) {
      crowd.set(marker.nodeId, (crowd.get(marker.nodeId) ?? 0) + 1);
    }
  }

  const tokens: CursorToken[] = [];

  for (const marker of markers) {
    const box = boxes.get(marker.nodeId);

    if (!box) {
      continue;
    }

    const lane = lanes.get(marker.nodeId) ?? 0;

    lanes.set(marker.nodeId, lane + 1);

    const width = box.width > 0 ? box.width : FALLBACK_NODE_WIDTH;
    const shared = crowd.get(marker.nodeId) ?? 1;
    const offset = (lane - (shared - 1) / 2) * TOKEN_LANE_GAP;

    tokens.push({
      ...marker,
      x: box.x + width / 2 + offset,
      y: box.y - TOKEN_LIFT,
    });
  }

  return tokens;
}

/**
 * bump the sequence number of every token that changed node since `previous`.
 *
 * the renderer restarts a token's jump animation by alternating between two identical keyframe
 * names, and this decides when to flip. a token that is merely *new* -- a fresh fork -- does not
 * bump: it has nowhere to have jumped from, so it should appear rather than fly in from a node it
 * was never on.
 */
export function advanceHopSequence(
  tokens: CursorToken[],
  previous: Map<string, string>,
  sequence: Map<string, number>,
): { sequence: Map<string, number>; positions: Map<string, string> } {
  const positions = new Map<string, string>();
  const next = new Map<string, number>();

  for (const token of tokens) {
    positions.set(token.id, token.nodeId);

    const before = previous.get(token.id);
    const carried = sequence.get(token.id) ?? 0;

    next.set(token.id, before !== undefined && before !== token.nodeId ? carried + 1 : carried);
  }

  return { sequence: next, positions };
}
