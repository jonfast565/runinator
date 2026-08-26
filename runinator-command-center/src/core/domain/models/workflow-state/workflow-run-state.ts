import type { JsonValue } from "../../json";
import type { CompensationFrame } from "./compensation-frame";
import type { ControlFrame } from "./control-frame";
import type { DebugFrame } from "./debug-frame";
import type { MapFrame } from "./map-frame";
import type { ParallelFrame } from "./parallel-frame";
import type { RaceFrame } from "./race-frame";
import type { RunCursor } from "./run-cursor";
import type { TryFrame } from "./try-frame";

/**
 * typed execution state assembled from normalized persistence tables.
 */
export interface WorkflowExecutionState {
  /**
   * where the run is on its track. one entry for a linear run; `parallel`/`race` fan out more, and
   * the debugger can add speculative branches.
   */
  cursors?: RunCursor[];
  control?: ControlFrame;
  debug?: DebugFrame;
  parallel?: ParallelFrame;
  map?: MapFrame;
  race?: RaceFrame;
  try?: TryFrame;
  compensation?: CompensationFrame;
  run_metadata?: JsonValue;
  watch_fired?: boolean;
}
