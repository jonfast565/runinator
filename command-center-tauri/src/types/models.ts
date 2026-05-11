export type JsonRecord = Record<string, any>;

export interface ScheduledTask {
  id: number | null;
  name: string;
  cron_schedule: string;
  action_name: string;
  action_function: string;
  timeout: number;
  next_execution: string | null;
  enabled: boolean;
  immediate: boolean;
  blackout_start: string | null;
  blackout_end: string | null;
  default_parameters: JsonRecord;
  mcp_enabled: boolean;
  metadata: JsonRecord;
  tags: string[];
}

export interface ActionMetadata {
  function_name: string;
  description?: string | null;
  parameters: ActionParameterMetadata[];
  results: ActionResultMetadata[];
}

export type ParameterValueType =
  | "string"
  | "integer"
  | "number"
  | "boolean"
  | "string_array"
  | "number_array"
  | "object"
  | "json";

export interface ActionParameterMetadata {
  name: string;
  value_type: ParameterValueType;
  label?: string | null;
  description?: string | null;
  required: boolean;
  default_value?: any;
  secret: boolean;
}

export interface ActionResultMetadata {
  name: string;
  value_type: ParameterValueType;
  label?: string | null;
  description?: string | null;
}

export interface ProviderRuntimeMetadata {
  credential_scopes: string[];
  contract?: string | null;
}

export interface ProviderMetadata {
  name: string;
  actions: ActionMetadata[];
  metadata: ProviderRuntimeMetadata;
}

export interface RunSummary {
  id: number;
  task_id?: number;
  workflow_id?: number;
  status: string;
  parameters?: JsonRecord;
  output_json?: any;
  message?: string | null;
  trigger?: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  workflow_run_id?: number | null;
  workflow_node_id?: string | null;
  active_node_id?: string | null;
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
  parameters: JsonRecord;
  output_json?: any;
  state?: JsonRecord;
  transition_reason?: string | null;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
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

export interface CredentialSummary {
  scope: string;
  name: string;
}

export interface WorkflowRunCreated {
  id: number;
}

export interface ServiceStatus {
  service_url: string | null;
}
