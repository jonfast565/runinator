export type JsonRecord = Record<string, any>;

export interface ScheduledTask {
  id: number | null;
  name: string;
  cron_schedule: string;
  action_name: string;
  action_function: string;
  action_configuration: string;
  timeout: number;
  next_execution: string | null;
  enabled: boolean;
  immediate: boolean;
  blackout_start: string | null;
  blackout_end: string | null;
  input_schema: JsonRecord;
  default_parameters: JsonRecord;
  output_schema?: JsonRecord | null;
  mcp_enabled: boolean;
  metadata: JsonRecord;
  tags: string[];
}

export interface RunSummary {
  id: number;
  task_id?: number;
  workflow_id?: number;
  status: string;
  trigger?: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  workflow_run_id?: number | null;
  workflow_node_id?: string | null;
}

export interface RunChunk {
  id: number;
  stream: string;
  content: string;
}

export interface RunArtifact {
  id: number;
  name: string;
  mime_type: string;
  size_bytes: number;
  uri: string;
  created_at: string;
}

export interface WorkflowDefinition {
  id: number | null;
  name: string;
  version: number;
  enabled: boolean;
  input_schema: JsonRecord;
  definition: JsonRecord;
}

export interface WorkflowNodeRun {
  id: number;
  workflow_run_id: number;
  node_id: string;
  task_run_id: number | null;
  status: string;
  attempt: number;
  message: string | null;
}

export interface WorkflowRunDetail {
  run: RunSummary & { workflow_id: number; message?: string | null };
  nodes: WorkflowNodeRun[];
}

export interface TaskResponse {
  success: boolean;
  message: string;
}

export interface SaveTaskResponse extends TaskResponse {
  creating: boolean;
}

export interface WorkflowRunCreated {
  id: number;
}

export interface ServiceStatus {
  service_url: string | null;
}
