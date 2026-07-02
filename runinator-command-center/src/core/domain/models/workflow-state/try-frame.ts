import type { JsonValue } from "../../json";

/** `state.try` / try node-run phase bookkeeping. */
export interface TryFrame {
  node_id: string;
  phase: string;
  pending_status?: string | null;
  pending_output?: JsonValue | null;
}
