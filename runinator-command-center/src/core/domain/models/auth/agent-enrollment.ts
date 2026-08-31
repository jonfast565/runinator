export interface AgentEnrollmentToken {
  token_id: string;
  org_id?: string | null;
  labels: Record<string, string>;
  service_url: string;
  spki_pin?: string | null;
  permanent: boolean;
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
  permanent: boolean;
}

export interface CreateAgentEnrollmentTokenResponse {
  enrollment_token: AgentEnrollmentToken;
  token: string;
}

export interface AgentMachineEnrollment {
  machine_id: string;
  instance_id: string;
  org_id?: string | null;
  permanent: boolean;
  disabled: boolean;
  credential_count: number;
  active_credential_count: number;
  enrolled_by?: string | null;
  enrolled_at: string;
  updated_at: string;
  last_used_at?: string | null;
}
