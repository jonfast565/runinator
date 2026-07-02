import type { JsonRecord } from "../../json";

export interface WorkflowRunArtifact {
  id: string;
  workflow_run_id: string;
  node_id: string;
  artifact_id: string;
  name: string;
  mime_type: string;
  size_bytes: number;
  uri: string;
  metadata?: JsonRecord;
  created_at: string;
}
