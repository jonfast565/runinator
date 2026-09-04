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
