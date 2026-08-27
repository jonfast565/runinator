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
  /** optional correlation key, matched by `await workflow ... key` joins. */
  correlation_key?: string | null;
  /** Parent pipeline execution when this workflow is a pipeline member. */
  pipeline_run_id?: string | null;
}
