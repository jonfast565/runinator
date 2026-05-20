export type JsonRecord = Record<string, any>;

export type WorkflowNodeKind =
  | "start"
  | "action"
  | "task"
  | "wait"
  | "condition"
  | "switch"
  | "approval"
  | "loop"
  | "parallel"
  | "join"
  | "try"
  | "map"
  | "race"
  | "emit"
  | "subflow"
  | "end"
  | "fail";

export type WorkflowDirectTransitionKey = "next" | "on_success" | "on_failure" | "on_timeout" | "on_reject";
export type WorkflowConnectionHandle = string;
export type WorkflowNodeId = string;
export interface WorkflowNodeRef {
  "$node": WorkflowNodeId;
}
export type WorkflowPathSegment = string | number;

export type WorkflowEditorEdgeKind = "direct" | "branch" | "control";

export interface WorkflowEdgeSemanticOption {
  id: string;
  label: string;
  description: string;
}

export interface WorkflowSemanticHandle {
  id: string;
  label: string;
  type: "source" | "target";
  semanticOptionId?: string;
}

export type WorkflowValidationSeverity = "error" | "warning";

export interface WorkflowValidationIssue {
  severity: WorkflowValidationSeverity;
  message: string;
  nodeId: string;
  edgeKey?: string;
}

export interface WorkflowInlineEditDescriptor {
  label: string;
  value: string;
  valueKind: "text" | "number";
}

export type WorkflowEdgeEditorMatchKind = "equals" | "not_equals" | "exists" | "when";

export interface WorkflowEdgeEditorDraft {
  edgeId: string;
  source: string;
  target: string;
  optionId: string;
  sourceHandle?: WorkflowConnectionHandle | null;
  targetHandle?: WorkflowConnectionHandle | null;
  label: string;
  whenJson: string;
  matchKind: WorkflowEdgeEditorMatchKind;
  matchJson: string;
  canEditLabel: boolean;
  canEditCondition: boolean;
  canEditSwitchCase: boolean;
  canMove: boolean;
  orderIndex: number;
  orderCount: number;
}

export interface WorkflowEditorEdgeData {
  kind: WorkflowEditorEdgeKind;
  transitionKey?: WorkflowDirectTransitionKey;
  branchIndex?: number;
  parameterKey?: string;
  parameterIndex?: number;
  sourceHandle?: WorkflowConnectionHandle;
  targetHandle?: WorkflowConnectionHandle;
  validationCount?: number;
  validationSeverity?: WorkflowValidationSeverity;
  validationMessages?: string[];
  editable: boolean;
}

export interface WorkflowEditorNodeRecord extends JsonRecord {
  id: string;
  kind: WorkflowNodeKind;
  transitions?: JsonRecord;
  parameters?: JsonRecord;
}

export interface WorkflowLayoutPosition {
  x: number;
  y: number;
}

export type WorkflowLayoutDirection = "horizontal" | "vertical";


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
  workflow_id?: number;
  workflow_snapshot?: WorkflowDefinition | null;
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
  state?: JsonRecord;
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

export interface ScheduledTask {
  id: number | null;
  name: string;
  cron_schedule: string;
  action_name: string;
  action_function: string;
  enabled: boolean;
  mcp_enabled?: boolean;
  timeout: number;
  configuration: JsonRecord;
}

export interface SaveTaskResponse {
  success: boolean;
  message: string;
  task?: ScheduledTask | null;
}

export interface WorkflowDefinition {
  id: number | null;
  name: string;
  version: number;
  enabled: boolean;
  input_schema: JsonRecord;
  definition: JsonRecord;
}

export type WorkflowTriggerKind = "cron" | "manual";

export interface WorkflowTrigger {
  id: number | null;
  workflow_id: number;
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

export interface WorkflowNodeRun {
  id: number;
  workflow_run_id: number;
  node_id: string;
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
  run: RunSummary & { workflow_id: number; workflow_snapshot?: WorkflowDefinition | null; message?: string | null };
  nodes: WorkflowNodeRun[];
}

export interface TaskResponse {
  success: boolean;
  message: string;
}


export interface WorkflowBundleSaveRequest {
  workflow: WorkflowDefinition;
  tasks?: ScheduledTask[];
}

export interface WorkflowBundleSaveResponse {
  workflow: WorkflowDefinition;
  tasks: any[];
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
