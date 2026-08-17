import type { JsonRecord } from "../json";

export type ReplicaKind =
  | "worker"
  | "waker"
  | "webservice"
  | "background"
  | "postgres"
  | "archiver";

export type ReplicaStatus = "live" | "stale" | "offline";

export type AgentConnectionState =
  | "stopped"
  | "registering"
  | "connecting"
  | "connected"
  | "draining"
  | "reconnecting"
  | "reenrollment_required";

export interface AgentStatusReport {
  connection_state: AgentConnectionState;
  reconnect_retry_seconds?: number | null;
  broker_mode: string;
  broker_endpoint: string;
  in_flight: number;
  succeeded: number;
  failed: number;
  timed_out: number;
  canceled: number;
  last_error?: string | null;
  last_error_at?: string | null;
  outbox_depth: number;
  agent_version?: string | null;
  config_hash: string;
  provider_count: number;
  labels: Record<string, string>;
  uptime_seconds: number;
  heartbeat_seq: number;
  clock_skew_ms: number;
  stale_after_seconds?: number | null;
}

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

/// one provider a replica advertises. the metadata is the provider's own, so it is typed loosely
/// here and read only for its name and action count.
export interface ReplicaProviderRegistration {
  replica_id: string;
  provider_name: string;
  provider: {
    name: string;
    actions: { function_name: string; description?: string | null }[];
    metadata: { credential_scopes: string[] };
  };
  created_at?: string;
  updated_at?: string;
}

export interface ReplicaCounts {
  workers: number;
  wakers: number;
  webservices: number;
  background: number;
}

export interface ReplicaListResponse {
  counts: ReplicaCounts;
  replicas: ReplicaRecord[];
}

export type AgentDirectiveState =
  | "pending"
  | "published"
  | "accepted"
  | "completed"
  | "failed"
  | "unsupported"
  | "expired";

export type AgentDirectiveKind =
  | { type: "diagnostics" }
  | { type: "tail_logs"; lines: number }
  | { type: "list_sandbox"; path: string }
  | { type: "fetch_file"; path: string; max_bytes: number }
  | { type: "drain" }
  | { type: "undrain" }
  | { type: "restart" };

export interface AgentDirectiveRecord {
  directive_id: string;
  replica_id: string;
  kind: AgentDirectiveKind | JsonRecord;
  state: AgentDirectiveState;
  issued_at: string;
  expires_at: string;
  published_at?: string | null;
  completed_at?: string | null;
  payload: unknown;
  message?: string | null;
  attempts: number;
}
