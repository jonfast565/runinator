export interface AgentEnrollmentToken {
  token_id: string;
  org_id?: string | null;
  labels: Record<string, string>;
  service_url: string;
  spki_pin?: string | null;
  expires_at: string;
  consumed_at?: string | null;
  issued_by?: string | null;
  created_at: string;
}

export interface CreateAgentEnrollmentTokenInput {
  ttl_seconds: number;
  org_id?: string | null;
  labels: Record<string, string>;
  service_url: string;
  cluster_id?: string | null;
  spki_pin?: string | null;
}

export interface CreateAgentEnrollmentTokenResponse {
  enrollment_token: AgentEnrollmentToken;
  token: string;
}
