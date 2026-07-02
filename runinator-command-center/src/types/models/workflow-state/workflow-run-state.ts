import type { JsonValue } from "../../json";
import type { CompensationFrame } from "./compensation-frame";
import type { ControlFrame } from "./control-frame";
import type { DebugFrame } from "./debug-frame";
import type { LoopFrame } from "./loop-frame";
import type { MapFrame } from "./map-frame";
import type { ParallelFrame } from "./parallel-frame";
import type { RaceFrame } from "./race-frame";
import type { TryFrame } from "./try-frame";

/**
 * typed view of `workflow_run.state`. mirrors runinator-models::WorkflowRunState;
 * unmodeled keys may still exist on the wire object beside these frames.
 */
export interface WorkflowRunState {
  control?: ControlFrame;
  debug?: DebugFrame;
  loop?: LoopFrame;
  parallel?: ParallelFrame;
  map?: MapFrame;
  race?: RaceFrame;
  try?: TryFrame;
  compensation?: CompensationFrame;
  run_metadata?: JsonValue;
  watch_fired?: boolean;
}
