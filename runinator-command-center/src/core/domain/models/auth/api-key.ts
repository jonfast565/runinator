export interface ApiKey {
  id: string | null;
  name: string;
  principal_kind: "user" | "service";
  principal_id: string;
  system_role?: "engine" | "worker" | "waker" | "agent" | "replica" | null;
  org_id?: string | null;
  action_ceiling: string[];
  key_prefix: string;
  last_used_at?: string | null;
  expires_at?: string | null;
  disabled: boolean;
  created_at: string;
}

export interface CreateApiKeyResponse {
  api_key: ApiKey;
  secret: string;
}
