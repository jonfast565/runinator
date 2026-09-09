export interface DurableWorkspace {
  permission: "view" | "run" | "edit" | "own";
  id: string;
  key: string;
  org_id: string | null;
  head_version: number;
  revision: number;
  created_at: string;
  updated_at: string;
}
export interface WorkspaceFile {
  path: string;
  size_bytes: number;
  sha256: string;
  executable: boolean;
  link_target?: string;
}
export interface WorkspaceSnapshot {
  workspace_id: string;
  version: number;
  parent_version: number;
  origin:
    | { kind: "workflow"; workflow_run_id: string; effect_id: string; attempt: number }
    | { kind: "import"; transfer_id: string; format: string };
  revision_id: string;
  usage: { logical_bytes: number; entries: number; results_bytes: number };
  limits: { max_bytes: number; max_entries: number; max_results_bytes: number };
  created_at: string;
}

export interface WorkspaceEntry {
  name: string;
  kind: "file" | "directory" | "symlink" | "result";
  inode_number: number;
  content_id: string;
  size_bytes: number;
  executable: boolean;
  link_target: string | null;
}
export interface WorkspaceDirectory {
  revision_id: string;
  path: string;
  entries: WorkspaceEntry[];
  next_cursor: string | null;
}

export interface WorkspaceDiff {
  before_revision: string;
  after_revision: string;
  changes: { path: string; result: boolean; before: string | null; after: string | null }[];
  next_cursor: string | null;
}

export interface WorkspaceTransfer {
  id: string;
  workspace_id: string;
  version: number;
  importing: boolean;
  state: string;
  bytes_processed: number;
  error: string | null;
  expires_at: string;
}
