import type { ProviderMetadata } from "../provider/provider-metadata";
import type { SettingKind } from "../setting";

export interface WdlDiagnostic {
  start: number;
  end: number;
  line: number;
  column: number;
  severity: "error" | "warning";
  message: string;
}

export interface WdlSettingRef {
  scope: string;
  name: string;
  kind: SettingKind;
}

export interface WdlCompletionRequest {
  source: string;
  cursor_byte: number;
  providers: ProviderMetadata[];
  settings: WdlSettingRef[];
}

export interface WdlCompletionItem {
  label: string;
  kind: string;
  detail?: string | null;
  documentation?: string | null;
  insert_text: string;
  is_snippet: boolean;
}

export interface WdlCompletionResponse {
  replace_start_byte: number;
  replace_end_byte: number;
  items: WdlCompletionItem[];
}

export interface WdlHoverRequest {
  source: string;
  cursor_byte: number;
  providers: ProviderMetadata[];
  settings?: WdlSettingRef[];
}

export interface WdlHoverResponse {
  range_start_byte: number;
  range_end_byte: number;
  title: string;
  kind: string;
  detail?: string | null;
  documentation?: string | null;
}
