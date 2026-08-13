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
 * marks a cursor as an interrupt handler rather than an ordinary thread of control.
 * mirrors runinator-models::InterruptFrame.
 */
export interface InterruptFrame {
  /** the cursor this handler suspended, and will hand control back to. */
  interrupted_cursor: string;
  /** what raised it: `wake`, `timeout`, `retry`, `failure`, `resolved`, `child`, `external`, `orphan_signal`. */
  source: string;
  payload?: JsonValue;
  /** where the suspended thread resumes. */
  resume?: { node_id?: string } | null;
  raised_at?: string;
}

/**
 * one position on a run's track. mirrors runinator-models::RunCursor.
 *
 * a linear run holds one; `parallel`/`race` fan out more, and the debugger can add speculative
 * ones. `debug` is the authoritative per-branch debugger state -- `execution_state.debug` is the
 * primary cursor's mirror.
 */
export interface RunCursor {
  id: string;
  node_id: string;
  /** the loops this cursor is inside, outermost first; one frame per nesting level. */
  loops?: LoopFrame[];
  try?: TryFrame;
  /** the fan-out node that forked this cursor, for branch cursors. */
  forked_by?: string | null;
  speculative?: SpeculativeFrame | null;
  debug?: DebugFrame | null;
  last_output?: JsonValue;
  /** set when this cursor is running an interrupt handler region. */
  interrupt?: InterruptFrame | null;
  /** the handler cursor that froze this one; a suspended thread is not advancing. */
  suspended_by?: string | null;
  /** seconds this thread spent frozen behind an interrupt at its current position. */
  suspended_seconds?: number;
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
  /** what raised the interrupt this cursor is handling, or null for an ordinary thread. */
  interruptSource: string | null;
  /** true while this thread is frozen behind an interrupt handler. */
  suspended: boolean;
}

/**
 * a fixed, colour-blind-safe rotation indexed by `CursorMarker.paletteIndex`.
 *
 * one table, because the rail, the node graph, and the travelling tokens all name the same branch
 * and have to agree on its colour by construction rather than by three copies staying in step.
 */
export const CURSOR_PALETTE = [
  "#3b82f6",
  "#f59e0b",
  "#10b981",
  "#a855f7",
  "#ef4444",
  "#14b8a6",
  "#eab308",
  "#ec4899",
] as const;

/** the colour for a cursor's palette slot. */
export function cursorColor(paletteIndex: number): string {
  return CURSOR_PALETTE[paletteIndex % CURSOR_PALETTE.length];
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
export function isInterruptHandler(cursor: RunCursor): boolean {
  return Boolean(cursor.interrupt);
}

export function isSuspended(cursor: RunCursor): boolean {
  return Boolean(cursor.suspended_by);
}

export function cursorLabel(cursor: RunCursor, index: number): string {
  const speculative = cursor.speculative;

  if (speculative) {
    const label = speculative.label?.trim();

    if (label) {
      return label;
    }

    return `what-if ${String(index + 1)}`;
  }

  // a handler is named for what raised it rather than where it stands: it is a side-channel, and
  // "wake handler" says more about why the branch exists than its entry node id does.
  if (cursor.interrupt) {
    return `${cursor.interrupt.source} handler`;
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
    interruptSource: cursor.interrupt?.source ?? null,
    suspended: isSuspended(cursor),
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
