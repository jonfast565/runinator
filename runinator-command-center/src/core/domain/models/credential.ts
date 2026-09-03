import type { JsonValue } from "../json";
import type { SettingKind } from "./setting";

export interface CredentialSummary {
  id?: string;
  org_id?: string | null;
  scope: string;
  name: string;
  kind?: SettingKind;
  expires_at?: string | null;
}

export interface CredentialDetail extends CredentialSummary {
  value?: JsonValue;
  schema?: JsonValue | null;
}
