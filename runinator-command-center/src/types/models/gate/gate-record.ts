import type { JsonRecord, JsonValue } from "../../json";
import type { GateKind } from "./gate-kind";

export interface GateRecord {
  id?: string | null;
  workflow_run_id: string;
  node_id: string;
  kind: GateKind;
  status: string;
  label?: string | null;
  condition?: JsonValue;
  reason?: string | null;
  resolved_by?: string | null;
  resolved_at?: string | null;
  metadata?: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}
