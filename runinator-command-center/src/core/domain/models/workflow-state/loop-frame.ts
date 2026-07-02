import type { JsonValue } from "../../json";

/** `state.loop` iteration bookkeeping for a loop body. */
export interface LoopFrame {
  index?: number;
  item?: JsonValue;
  return_to?: string;
}
