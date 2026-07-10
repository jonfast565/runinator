import type { JsonRecord } from "../json";

export type ReplicaKind =
  | "worker"
  | "waker"
  | "webservice"
  | "background"
  | "postgres"
  | "archiver";

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
  background: number;
}

export interface ReplicaListResponse {
  counts: ReplicaCounts;
  replicas: ReplicaRecord[];
}
