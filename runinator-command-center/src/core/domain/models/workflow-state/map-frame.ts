import type { JsonValue } from "../../json";

export interface MapChild {
  index: number;
  child_run_id: string;
}

/** `state.map` bookkeeping (parent or child run). */
export interface MapFrame {
  node_id: string;
  target: string;
  items?: JsonValue[];
  concurrency?: number;
  next_index?: number;
  in_flight?: MapChild[];
  results?: JsonValue[];
  done?: number;
  item?: JsonValue | null;
  index?: number;
}
