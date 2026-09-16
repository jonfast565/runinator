export interface AiTokenUsage {
  input_tokens: number;
  cached_input_tokens: number;
  cache_creation_input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
}

export type AiCostSource = "provider_reported" | "rate_card";

export interface AiUsageRecord {
  event_id: string;
  effect_id: string;
  workflow_run_id: string;
  workflow_id: string;
  node_id: string | null;
  attempt: number;
  provider: string;
  model: string;
  tokens: AiTokenUsage;
  cost_microusd: number | null;
  cost_source: AiCostSource | null;
  recorded_at: string;
}

export interface AiUsageTotals {
  requests: number;
  unpriced_requests: number;
  tokens: AiTokenUsage;
  cost_microusd: number | null;
}

export interface AiUsageBreakdown {
  key: string;
  totals: AiUsageTotals;
}

export interface AiUsageReport {
  totals: AiUsageTotals;
  records: AiUsageRecord[];
  by_run: AiUsageBreakdown[];
  by_node: AiUsageBreakdown[];
  by_provider: AiUsageBreakdown[];
  by_model: AiUsageBreakdown[];
}

export interface AiRateEntry {
  provider: string;
  model: string;
  input_microusd_per_million_tokens: number;
  cached_input_microusd_per_million_tokens: number;
  cache_creation_input_microusd_per_million_tokens: number;
  output_microusd_per_million_tokens: number;
  reasoning_microusd_per_million_tokens: number;
}
