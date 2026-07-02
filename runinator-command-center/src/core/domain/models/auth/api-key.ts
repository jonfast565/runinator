export interface ApiKey {
  id: string | null;
  name: string;
  user_id?: string | null;
  is_service: boolean;
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
