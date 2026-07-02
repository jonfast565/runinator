import type { JsonValue } from "../json";
import type { SettingKind } from "./setting";

export interface CredentialSummary {
  scope: string;
  name: string;
  kind?: SettingKind;
}

export interface CredentialDetail extends CredentialSummary {
  value?: JsonValue;
  secret?: JsonValue;
}
