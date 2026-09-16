//! ai token pricing and rate-card selection contracts.

use super::*;
use crate::ai_usage::{AiRateEntry, AiTokenUsage, AiUsageTotals};

fn rate(provider: &str, model: &str, input: u64, output: u64) -> AiRateEntry {
    AiRateEntry {
        provider: provider.into(),
        model: model.into(),
        input_microusd_per_million_tokens: input,
        cached_input_microusd_per_million_tokens: 0,
        cache_creation_input_microusd_per_million_tokens: 0,
        output_microusd_per_million_tokens: output,
        reasoning_microusd_per_million_tokens: 0,
    }
}

#[test]
fn rate_card_prefers_exact_model_then_provider_wildcard() {
    let card = billing::RateCard {
        entries: Vec::new(),
        ai_entries: vec![
            rate("openai", "*", 1_000_000, 2_000_000),
            rate("openai", "gpt-test", 3_000_000, 4_000_000),
        ],
    };
    let tokens = AiTokenUsage {
        input_tokens: 2,
        output_tokens: 3,
        ..Default::default()
    };

    assert_eq!(card.price_ai_usage("openai", "gpt-test", &tokens), Some(18));
    assert_eq!(card.price_ai_usage("openai", "other", &tokens), Some(8));
    assert_eq!(card.price_ai_usage("anthropic", "other", &tokens), None);
}

#[test]
fn ai_price_rounds_fractional_microusd_without_floating_point() {
    let entry = rate("openai", "gpt-test", 500_000, 0);
    assert_eq!(
        entry.price(&AiTokenUsage {
            input_tokens: 3,
            ..Default::default()
        }),
        2
    );
}

#[test]
fn legacy_rate_card_defaults_to_no_ai_prices() {
    let card: billing::RateCard = serde_json::from_str(r#"{"entries":[]}"#).unwrap();
    assert!(card.ai_entries.is_empty());
}

#[test]
fn unpriced_totals_serialize_as_an_explicit_unknown_cost() {
    let json = serde_json::to_value(AiUsageTotals::default()).unwrap();
    assert!(
        json.get("cost_microusd")
            .is_some_and(serde_json::Value::is_null)
    );
}
