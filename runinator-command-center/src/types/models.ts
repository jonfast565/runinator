export type JsonRecord = Record<string, any>;

export type WorkflowNodeKind =
  | "start"
  | "action"
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
  | "config"
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
export type WorkflowEdgeStyle = "bezier" | "straight" | "square";

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
  edgeStyle: WorkflowEdgeStyle;
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

export interface WorkflowEdgeLabelOffset {
  x: number;
  y: number;
}

export interface WorkflowEditorEdgeData {
  kind: WorkflowEditorEdgeKind;
  transitionKey?: WorkflowDirectTransitionKey;
  branchIndex?: number;
  parameterKey?: string;
  parameterIndex?: number;
  sourceHandle?: WorkflowConnectionHandle;
  targetHandle?: WorkflowConnectionHandle;
  edgeStyle?: WorkflowEdgeStyle;
  labelOffset?: WorkflowEdgeLabelOffset | null;
  parallelOffset?: number;
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

export type RuninatorType =
  | { type: "null" }
  | { type: "boolean" }
  | { type: "integer" }
  | { type: "number" }
  | { type: "string" }
  | { type: "array"; items: RuninatorType }
  | { type: "map"; values: RuninatorType }
  | { type: "struct"; fields: Record<string, RuninatorField>; additional?: RuninatorType }
  | { type: "union"; variants: RuninatorType[] }
  | { type: "any" };

export interface RuninatorField {
  ty: RuninatorType;
  required: boolean;
}

export interface ActionParameterMetadata {
  name: string;
  ty: RuninatorType;
  label?: string | null;
  description?: string | null;
  required: boolean;
  default_value?: any;
  secret: boolean;
}

export interface ActionResultMetadata {
  name: string;
  ty: RuninatorType;
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
  name?: string | null;
}

export interface RunChunk {
  id: number;
  stream: string;
  content: string;
}

export interface RunArtifact {
  id: number;
  run_id: number;
  workflow_node_run_id?: number | null;
  name: string;
  mime_type: string;
  size_bytes: number;
  uri: string;
  metadata?: JsonRecord;
  created_at: string;
}

export type NotificationChannel = "in_app" | "email" | "slack";
export type NotificationSeverity = "info" | "success" | "warning" | "error";

export interface Notification {
  id: number;
  workflow_run_id?: number | null;
  workflow_node_id?: string | null;
  channel: NotificationChannel | string;
  severity: NotificationSeverity | string;
  title: string;
  body?: string | null;
  target?: string | null;
  metadata?: JsonRecord;
  read_at?: string | null;
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
  input_type: RuninatorType;
  definition: JsonRecord;
}

export interface WorkflowBundle {
  workflows: WorkflowDefinition[];
  triggers: WorkflowTrigger[];
}

export interface WdlDiagnostic {
  start: number;
  end: number;
  line: number;
  column: number;
  severity: "error" | "warning";
  message: string;
}

export interface WdlSettingRef {
  scope: string;
  name: string;
  kind: SettingKind;
}

export interface WdlCompletionRequest {
  source: string;
  cursor_byte: number;
  providers: ProviderMetadata[];
  settings: WdlSettingRef[];
}

export interface WdlCompletionResponse {
  replace_start_byte: number;
  replace_end_byte: number;
  items: WdlCompletionItem[];
}

export interface WdlCompletionItem {
  label: string;
  kind: string;
  detail?: string | null;
  documentation?: string | null;
  insert_text: string;
  is_snippet: boolean;
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


export type SettingKind = "secret" | "config";

export interface CredentialSummary {
  scope: string;
  name: string;
  kind?: SettingKind;
}

export interface WorkflowRunCreated {
  id: number;
}

export interface ServiceStatus {
  service_url: string | null;
}

export type ReplicaKind = "worker" | "waker" | "webservice";

export type ReplicaStatus = "live" | "stale" | "offline";

export interface ReplicaRecord {
  replica_id: number;
  replica_type: ReplicaKind;
  instance_id: string;
  runtime_id: string;
  status: ReplicaStatus;
  display_name?: string | null;
  host?: string | null;
  port?: number | null;
  base_path?: string | null;
  observed_ip?: string | null;
  attributes: JsonRecord;
  first_seen_at: string;
  last_heartbeat_at: string;
  last_seen_at: string;
  offline_at?: string | null;
}

export interface ReplicaCounts {
  workers: number;
  wakers: number;
  webservices: number;
}

export interface ReplicaListResponse {
  counts: ReplicaCounts;
  replicas: ReplicaRecord[];
}

export interface DevPackFile {
  path: string;
  kind: string;
  size_bytes?: number | null;
  modified_at?: string | null;
}

export interface DevPackInspectResult {
  path: string;
  files: DevPackFile[];
  workflows: WorkflowDefinition[];
  triggers: WorkflowTrigger[];
  settings_count: number;
  settings: WdlSettingRef[];
}

export interface DevPackTextFile {
  path: string;
  content: string;
  modified_at?: string | null;
}

export interface DevPackApplyResult {
  path: string;
  files: DevPackFile[];
  imported: {
    workflows: WorkflowBundle;
    secrets?: {
      secrets?: unknown[];
    };
  };
}
