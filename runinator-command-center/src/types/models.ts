export type JsonRecord = Record<string, any>;

export type WorkflowNodeKind =
  | "start"
  | "action"
  | "wait"
  | "condition"
  | "switch"
  | "approval"
  | "gate"
  | "signal"
  | "loop"
  | "parallel"
  | "join"
  | "try"
  | "map"
  | "race"
  | "output"
  | "input"
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
  labelAnchor: number;
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
  // selection priority for predicate edges; lower numbers are evaluated first. null means unset.
  priority: number | null;
  canEditPriority: boolean;
}

export interface WorkflowEdgeLabelOffset {
  x: number;
  y: number;
}

export interface WorkflowEdgeLabelAnchor {
  position: number;
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
  labelAnchor?: WorkflowEdgeLabelAnchor | null;
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
  id: string;
  workflow_id?: string;
  workflow_snapshot?: WorkflowDefinition | null;
  status: string;
  parameters?: JsonRecord;
  output_json?: any;
  message?: string | null;
  trigger?: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  workflow_run_id?: string | null;
  workflow_node_id?: string | null;
  active_node_id?: string | null;
  state?: JsonRecord;
  name?: string | null;
}

export interface RunChunk {
  id: string;
  stream: string;
  content: string;
}

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

export interface WorkflowRunDeliverable {
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

export type NotificationChannel = "in_app" | "email" | "slack";
export type NotificationSeverity = "info" | "success" | "warning" | "error";

export interface Notification {
  id: string;
  workflow_run_id?: string | null;
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
  id: string | null;
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
  id: string | null;
  name: string;
  // semantic version string, e.g. "1.2.0".
  version: string;
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
  id: string | null;
  workflow_id: string;
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

export interface GateRecord {
  id?: string | null;
  workflow_run_id: string;
  node_id: string;
  kind: "manual" | "condition" | "external" | string;
  status: string;
  label?: string | null;
  condition?: JsonRecord | unknown;
  reason?: string | null;
  resolved_by?: string | null;
  resolved_at?: string | null;
  metadata?: JsonRecord;
  created_at?: string | null;
  updated_at?: string | null;
}

export interface WorkflowNodeRun {
  id: string;
  workflow_run_id: string;
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
  run: RunSummary & { workflow_id: string; workflow_snapshot?: WorkflowDefinition | null; message?: string | null };
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
  id: string;
}

export interface ServiceStatus {
  service_url: string | null;
}

export type ReplicaKind = "worker" | "waker" | "webservice";

export type ReplicaStatus = "live" | "stale" | "offline";

export interface ReplicaRecord {
  replica_id: string;
  replica_type: ReplicaKind;
  instance_id: string;
  runtime_id: string;
  status: ReplicaStatus;
  display_name?: string | null;
  host?: string | null;
  port?: number | null;
  base_path?: string | null;
  observed_ip?: string | null;
  version?: string | null;
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
