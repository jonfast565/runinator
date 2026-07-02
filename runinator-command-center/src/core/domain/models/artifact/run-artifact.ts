import type { JsonRecord } from "../../json";

export interface RunArtifact {
  id: string;
  run_id: string;
  workflow_node_run_id?: string | null;
  name: string;
  mime_type: string;
  size_bytes: number;
  uri: string;
  metadata?: JsonRecord;
  created_at: string;
}
