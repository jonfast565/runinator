import type { JsonValue } from "../json";

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
  workflow_run_id: string;
  effect_id: string;
  attempt: number;
  compressed_bytes: number;
  archive_sha256: string;
  files: WorkspaceFile[];
  results: Record<string, JsonValue>;
  created_at: string;
}
