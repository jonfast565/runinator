import type { JsonValue } from "../../json";

export type OrchestrationStatus =
  | "pending"
  | "running"
  | "waiting"
  | "suspended"
  | "completed"
  | "failed"
  | "terminated";

export interface OrchestrationBinding {
  id: string;
  admission_id: string;
  org_id?: string | null;
  scope: string;
  correlation_key: string;
  generation: number;
  pipeline_id: string;
  pipeline_revision: number;
  pipeline_digest: string;
  adapter_id?: string | null;
  adapter_revision?: number | null;
  policy: OrchestrationPolicy;
  status: OrchestrationStatus;
  current_phase?: string | null;
  current_attempt: number;
  current_epoch: number;
  restart_member?: string | null;
  resume_existing_epoch: boolean;
  subject_revision?: string | null;
  resources: JsonValue;
  budgets: Record<string, number>;
  last_reduced_sequence: number;
  version: number;
  reducer_lease_owner?: string | null;
  reducer_leased_until?: string | null;
  created_at: string;
  updated_at: string;
  finished_at?: string | null;
}

export interface OrchestrationPolicy {
  intents: Record<string, IntentPolicy>;
  phases: Record<string, PhasePolicy>;
  budgets: Record<string, { attempts: number; exhausted: "fail" | "pause" | "terminate" }>;
  defaults: JsonValue;
}

export interface IntentPolicy {
  effect: "terminate" | "suspend" | "resume" | "supersede" | "observe" | "signal";
  priority: number;
  coalesce_seconds?: number | null;
  stop?: "pause" | "cancel" | "none";
  restart?: { kind: "entry" | "current" | "member"; member?: string };
  subject_revision_pointer?: string | null;
  allow_self_originated?: boolean;
  signal_name?: string | null;
}

export interface ResultMapping {
  subject_revision?: string | null;
  resources?: string | null;
  evidence?: string | null;
  failure_class?: string | null;
}

export interface WorkspacePolicy {
  scope: string;
  requirements: JsonValue;
  lease_seconds: number;
  reuse: boolean;
  recovery: "replace" | "wait" | "fail";
}

export interface WorkspaceLease {
  id: string;
  admission_id: string;
  generation: number;
  scope: string;
  attempt: number;
  worker_instance_id: string;
  worker_replica_id?: string | null;
  local_key: string;
  requirements: JsonValue;
  status: "allocating" | "active" | "finalizing" | "released" | "abandoned";
  version: number;
  leased_until: string;
  unavailable_since?: string | null;
  evidence: JsonValue;
  created_at: string;
  updated_at: string;
}

export interface PhasePolicy {
  result: ResultMapping;
  workspace?: WorkspacePolicy | null;
}

export type IngressLifecycle = "unbound" | "active" | "terminal";
export type IngressAction = "start" | "interrupt" | "queue" | "record" | "requeue" | "dispatch";
export type IngressPredicateOperator = "equal" | "not_equal" | "in" | "contains" | "exists";

export interface IngressPredicate {
  pointer: string;
  operator: IngressPredicateOperator;
  value?: JsonValue;
}

export interface IngressRoute {
  event_type: string;
  lifecycle: IngressLifecycle;
  action: IngressAction;
  predicates: IngressPredicate[];
  intent?: string | null;
}

export interface IngressPolicy {
  scope: string;
  routes: IngressRoute[];
}

export interface OrchestrationEpoch {
  id: string;
  binding_id: string;
  epoch: number;
  pipeline_run_id?: string | null;
  start_member?: string | null;
  parameters: JsonValue;
  status: string;
  reason: string;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
}

export interface OrchestrationReduction {
  id: string;
  binding_id: string;
  inbox_event_id: string;
  sequence: number;
  matched_intents: string[];
  winner?: string | null;
  suppressed_intents: string[];
  binding_version: number;
  disposition: string;
  detail: JsonValue;
  created_at: string;
}

export interface OrchestrationEvidence {
  id: string;
  binding_id: string;
  epoch?: number | null;
  kind: string;
  subject_revision?: string | null;
  payload: JsonValue;
  source_event_id?: string | null;
  created_at: string;
}

export interface OrchestrationCommand {
  id: string;
  binding_id: string;
  epoch: number;
  command_type: string;
  operation_key: string;
  payload: JsonValue;
  status: string;
  attempts: number;
  result: JsonValue;
  created_at: string;
  updated_at: string;
}

export interface AdapterConfigurationField {
  name: string;
  value_type: unknown;
  required: boolean;
  secret: boolean;
  description?: string | null;
  default: JsonValue;
}

export interface AdapterKindMetadata {
  kind: string;
  version: string;
  display_name: string;
  description?: string | null;
  fields: AdapterConfigurationField[];
  event_names: string[];
  canonical_pointers: string[];
  capabilities: string[];
}

export interface AdapterDefinition {
  id: string;
  org_id: string;
  name: string;
  kind: string;
  current_revision: number;
  enabled: boolean;
  endpoint_identity: string;
  has_admitted_binding: boolean;
  created_at: string;
  updated_at: string;
}

export interface AdapterRevision {
  id: string;
  adapter_id: string;
  revision: number;
  kind_version: string;
  configuration: JsonValue;
  secret_bindings: Record<string, string>;
  identity_configuration: JsonValue;
  created_at: string;
  actor_id?: string | null;
}

export type DeliverySemantics = "at_least_once" | "idempotent" | "reconcilable";
export type ExternalOperationStatus = "pending" | "running" | "waiting" | "succeeded" | "failed";

export interface ExternalOperation {
  id: string;
  binding_id: string;
  epoch: number;
  workflow_run_id?: string | null;
  effect_id?: string | null;
  operation_key: string;
  provider: string;
  action: string;
  semantics: DeliverySemantics;
  attempt: number;
  status: ExternalOperationStatus;
  ambiguous: boolean;
  provenance: JsonValue;
  receipt: JsonValue;
  created_at: string;
  updated_at: string;
}
