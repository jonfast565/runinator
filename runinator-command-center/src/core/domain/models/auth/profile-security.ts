import type { Action } from "./action";
import type { ApiKey } from "./api-key";

export interface AuthSessionSummary {
  id: string;
  user_agent?: string | null;
  ip_address?: string | null;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  current: boolean;
}

export interface UpdateCurrentUserInput {
  email?: string | null;
}

export interface ChangePasswordInput {
  current_password: string;
  new_password: string;
}

export interface CreatePersonalApiKeyInput {
  name: string;
  org_id?: string | null;
  action_ceiling: Action[];
  expires_at?: string | null;
}

export interface PersonalApiKeySecret {
  api_key: ApiKey;
  secret: string;
}

export interface PersonalApiKeyScope {
  org_id: string | null;
  name: string;
  actions: Action[];
}
