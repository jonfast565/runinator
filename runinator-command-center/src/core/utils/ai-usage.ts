import type { AiTokenUsage } from "../domain/models";

export function aiTokenTotal(tokens: AiTokenUsage): number {
  return (
    tokens.input_tokens +
    tokens.cached_input_tokens +
    tokens.cache_creation_input_tokens +
    tokens.output_tokens +
    tokens.reasoning_tokens
  );
}

export function formatAiCost(costMicrousd: number | null): string {
  return costMicrousd === null ? "Unpriced" : `$${(costMicrousd / 1_000_000).toFixed(6)}`;
}
