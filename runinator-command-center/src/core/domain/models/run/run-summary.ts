import type { JsonRecord, JsonValue } from "../../json";

export interface RunSummary {
  id: string;
  workflow_id?: string;
  workflow_snapshot?: JsonRecord | null;
  status: string;
  parameters?: JsonRecord;
  output_json?: JsonValue;
  message?: string | null;
  trigger?: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  workflow_run_id?: string | null;
  workflow_node_id?: string | null;
  active_node_id?: string | null;
  state?: JsonRecord;
  name?: string | null;
}
