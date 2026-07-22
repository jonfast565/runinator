import type { JsonRecord } from "../../json";
import type { WorkflowTriggerKind } from "../workflow/trigger";

// a pipeline-level trigger. cron/manual start a run of this pipeline; a `chained` trigger is
// target-keyed (this pipeline starts when the configured source reaches a terminal state, carried in
// `configuration` as `source_workflow`/`source_pipeline` + `on`).
export interface PipelineTrigger {
  id: string | null;
  pipeline_id: string;
  kind: WorkflowTriggerKind;
  enabled: boolean;
  configuration: JsonRecord;
  next_execution: string | null;
  blackout_start: string | null;
  blackout_end: string | null;
  metadata: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}
