export interface FileDescriptor {
  id: string;
  name: string;
  path: string;
  mime_type: string;
  size_bytes: number;
  sha256: string;
}

export type WorkflowFileScope = "staged" | "library" | "run";

export interface WorkflowFile {
  descriptor: FileDescriptor;
  scope: WorkflowFileScope;
  org_id: string | null;
  owner_id: string | null;
  workflow_run_id: string | null;
  revision: number;
  current: boolean;
  archived: boolean;
  created_at: string;
}
