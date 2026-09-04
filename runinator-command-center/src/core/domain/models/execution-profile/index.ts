export interface ExecutionProfileCommand {
  argv: string[];
  interactive?: boolean;
}

export type ExecutionProfileSource =
  | { type: "file"; path: string; target: string }
  | { type: "directory"; path: string; glob?: string; target: string }
  | { type: "command"; command: ExecutionProfileCommand; target: string };

export interface ExecutionProfileInput {
  name: string;
  description: string;
  credential_scopes: string[];
  collection: {
    version: number;
    probe?: ExecutionProfileCommand | null;
    refresh?: ExecutionProfileCommand | null;
    sources: ExecutionProfileSource[];
  };
  exposure: {
    version: number;
    home_overlay: boolean;
    environment: Record<string, string>;
  };
  enabled: boolean;
}

export interface ExecutionProfile extends ExecutionProfileInput {
  id: string;
  org_id: string | null;
  config_version: number;
  config_digest: string;
  current_revision: number | null;
  current_digest: string | null;
  current_publisher_id: string | null;
  published_at: string | null;
  expires_at: string | null;
  refresh_requested_at: string | null;
  health: "unpublished" | "testing" | "ready" | "expiring" | "expired" | "error" | "disabled";
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

export type ExecutionProfileApprovalState = "approved" | "approval_required";
export type ExecutionProfileOperationKind = "dry_run" | "refresh";
export type ExecutionProfileOperationState = "queued" | "running" | "succeeded" | "failed";

/** One desktop's latest observation of a single approved profile configuration. */
export interface ExecutionProfileAgentStatus {
  profile_id: string;
  agent_id: string;
  config_digest: string;
  approval: ExecutionProfileApprovalState;
  last_seen_at: string;
  last_attempt_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
}

/** A durable operator request for an approved desktop collector. */
export interface ExecutionProfileOperation {
  id: string;
  profile_id: string;
  config_digest: string;
  kind: ExecutionProfileOperationKind;
  state: ExecutionProfileOperationState;
  requested_at: string;
  requested_by: string | null;
  claimed_by: string | null;
  started_at: string | null;
  lease_expires_at: string | null;
  completed_at: string | null;
  error: string | null;
}

/** Publication availability plus the independent desktop collection health. */
export interface ExecutionProfileCollectionStatus {
  profile_id: string;
  config_digest: string;
  publication_health: ExecutionProfile["health"];
  current_revision: number | null;
  published_at: string | null;
  expires_at: string | null;
  latest_operation: ExecutionProfileOperation | null;
  agents: ExecutionProfileAgentStatus[];
}
