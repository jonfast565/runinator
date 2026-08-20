import type { ProviderMetadata } from "../provider/provider-metadata";
import type { SettingKind } from "../setting";

export interface RexRapDiagnostic {
  start: number;
  end: number;
  line: number;
  column: number;
  severity: "error" | "warning";
  message: string;
}

export interface RexRapSettingRef {
  scope: string;
  name: string;
  kind: SettingKind;
}

export interface RexRapCompletionRequest {
  source: string;
  cursor_byte: number;
  providers: ProviderMetadata[];
  settings: RexRapSettingRef[];
}

export interface RexRapCompletionItem {
  label: string;
  kind: string;
  detail?: string | null;
  documentation?: string | null;
  insert_text: string;
  is_snippet: boolean;
}

export interface RexRapCompletionResponse {
  replace_start_byte: number;
  replace_end_byte: number;
  items: RexRapCompletionItem[];
}

export interface RexRapHoverRequest {
  source: string;
  cursor_byte: number;
  providers: ProviderMetadata[];
  settings?: RexRapSettingRef[];
}

export interface RexRapHoverResponse {
  range_start_byte: number;
  range_end_byte: number;
  title: string;
  kind: string;
  detail?: string | null;
  documentation?: string | null;
}
