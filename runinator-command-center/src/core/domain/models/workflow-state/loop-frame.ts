/**
 * one live loop on a cursor: which loop node, and where that loop is.
 *
 * mirrors `runinator-models`' `LoopFrame`. a cursor carries a stack of these (`RunCursor.loops`),
 * one per loop it is inside, so nested loops each keep their own lap.
 */
export interface LoopFrame {
  /** the loop node this frame belongs to; the key nested loops are told apart by. */
  node_id?: string;
  /** the iteration whose body is running now, zero-based. */
  index?: number;
  /** this loop's own node run for the current lap. */
  last_node_run_id?: string;
}
