import type { JsonRecord } from "../../json";
import type { Pipeline } from "./pipeline";

// a first-class pipeline execution: an orchestration envelope over the member workflow runs it
// started. `status` reuses the workflow-run status vocabulary (queued/running/waiting/terminal).
export interface PipelineRun {
  id: string;
  pipeline_id: string;
  pipeline_snapshot?: Pipeline | null;
  status: string;
  parameters: JsonRecord;
  state: JsonRecord;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  message?: string | null;
  trigger_source_kind?: string | null;
  trigger_actor_type?: string | null;
  trigger_actor_replica_id?: string | null;
  trigger_actor_display_name?: string | null;
  trigger_metadata?: JsonRecord;
}
