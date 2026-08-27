import type { JsonValue } from "../json";
import type { SettingKind } from "./setting";

export interface CredentialSummary {
  id?: string;
  scope: string;
  name: string;
  kind?: SettingKind;
}

export interface CredentialDetail extends CredentialSummary {
  value?: JsonValue;
}
