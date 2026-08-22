import type { JsonRecord } from "../../json";

export interface WorkflowRunArtifact {
  id: string;
  workflow_run_id: string;
  node_id: string;
  /** VM effect output has no legacy run_artifact id and therefore cannot use the old download route. */
  artifact_id: string | null;
  name: string;
  mime_type: string;
  size_bytes: number;
  uri: string;
  metadata?: JsonRecord;
  created_at: string;
}
