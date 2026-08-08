import type { JsonValue } from "../../json";
import type { DebugFrame } from "./debug-frame";
import type { LoopFrame } from "./loop-frame";
import type { TryFrame } from "./try-frame";

/**
 * marks a cursor as a debugger "what if" branch rather than a real thread of control.
 * mirrors runinator-models::SpeculativeFrame.
 */
export interface SpeculativeFrame {
  forked_from_cursor: string;
  label?: string | null;
  created_at?: string;
  /** nodes the operator armed for real dispatch; everything else shadows. */
  armed_nodes?: string[];
  /** merge-patch overlaid on this branch's context. */
  context_patch?: JsonValue;
}

/**
 * one position on a run's track. mirrors runinator-models::RunCursor.
 *
 * a linear run holds one; `parallel`/`race` fan out more, and the debugger can add speculative
 * ones. `debug` is the authoritative per-branch debugger state -- `run.state.debug` is only the
 * primary cursor's mirror.
 */
export interface RunCursor {
  id: string;
  node_id: string;
  loop?: LoopFrame;
  try?: TryFrame;
  /** the fan-out node that forked this cursor, for branch cursors. */
  forked_by?: string | null;
  speculative?: SpeculativeFrame | null;
  debug?: DebugFrame | null;
  last_output?: JsonValue;
}

/** how a cursor is drawn on the graph and named in the rail. */
export interface CursorMarker {
  id: string;
  nodeId: string;
  /** stable palette slot, derived once so the rail and the node card agree by construction. */
  paletteIndex: number;
  label: string;
  paused: boolean;
  speculative: boolean;
  /** does this speculative branch dispatch the node it is standing on for real? */
  armed: boolean;
  selected: boolean;
}

/** is this cursor a debugger "what if" branch? */
export function isSpeculative(cursor: RunCursor): boolean {
  return Boolean(cursor.speculative);
}

/**
 * is the cursor's current node armed for real dispatch?
 *
 * arming is per node, not per branch, so this answers only for where the branch is standing now --
 * which is the only node the rail can offer to arm. a real cursor is always "armed" in the sense
 * that it never shadows, but the control is speculative-only, so this reports false for it.
 */
export function isArmedHere(cursor: RunCursor): boolean {
  const speculative = cursor.speculative;

  if (!speculative) {
    return false;
  }

  return (speculative.armed_nodes ?? []).includes(cursor.node_id);
}

/**
 * the debugger runtime governing one cursor.
 *
 * falls back to the run-scoped frame only while *no* cursor carries one of its own -- a run
 * persisted before per-cursor debug state. once any cursor has been written the flat frame is the
 * primary's mirror, so a sibling without one is simply not under the debugger. this mirrors
 * `WorkflowRunState::cursor_debug` exactly; disagreeing with it would make the ui show branches as
 * paused that the reducer will happily keep running.
 */
export function cursorDebug(
  cursors: RunCursor[],
  cursorId: string,
  runFrame?: DebugFrame | null,
): DebugFrame | null {
  const own = cursors.find((cursor) => cursor.id === cursorId)?.debug;

  if (own) {
    return own;
  }

  if (cursors.some((cursor) => cursor.debug)) {
    return null;
  }

  return runFrame ?? null;
}

/** is this cursor parked under the debugger? */
export function isCursorPaused(
  cursors: RunCursor[],
  cursorId: string,
  runFrame?: DebugFrame | null,
): boolean {
  return Boolean(cursorDebug(cursors, cursorId, runFrame)?.paused);
}

/**
 * a short human name for a branch: its speculative label, else the fan-out that forked it, else
 * `main` for the run's original thread of control.
 */
export function cursorLabel(cursor: RunCursor, index: number): string {
  const speculative = cursor.speculative;

  if (speculative) {
    return speculative.label?.trim() || `what-if ${index + 1}`;
  }

  if (cursor.forked_by) {
    return `${cursor.forked_by}:${cursor.node_id}`;
  }

  return index === 0 ? "main" : cursor.node_id;
}

/**
 * project the run's cursors into draw-ready markers, ordered as persisted so palette slots stay
 * stable while a run advances.
 */
export function buildCursorMarkers(
  cursors: RunCursor[],
  runFrame?: DebugFrame | null,
  selectedCursorId?: string | null,
): CursorMarker[] {
  return cursors.map((cursor, index) => ({
    id: cursor.id,
    nodeId: cursor.node_id,
    paletteIndex: index,
    label: cursorLabel(cursor, index),
    paused: isCursorPaused(cursors, cursor.id, runFrame),
    speculative: isSpeculative(cursor),
    armed: isArmedHere(cursor),
    selected: cursor.id === selectedCursorId,
  }));
}

/** markers grouped by the node they sit on; a node may carry several. */
export function cursorsByNode(markers: CursorMarker[]): Map<string, CursorMarker[]> {
  const byNode = new Map<string, CursorMarker[]>();

  for (const marker of markers) {
    const list = byNode.get(marker.nodeId);

    if (list) {
      list.push(marker);
    } else {
      byNode.set(marker.nodeId, [marker]);
    }
  }

  return byNode;
}
