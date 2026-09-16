use std::sync::Arc;

use runinator_models::{
    ai_usage::{AiTokenUsage, AiUsage},
    runs::ProviderExecutionEvent,
    value::Value,
};
use runinator_plugin::provider::ProviderEventSink;

fn token_at(value: &Value, pointers: &[&str]) -> u64 {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
        .unwrap_or_default()
}

fn usd_to_microusd(value: &Value) -> Option<u64> {
    let raw = match value {
        Value::Number(number) => number.to_string(),
        Value::String(value) => value.clone(),
        _ => return None,
    };
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = if let Some((mantissa, exponent)) = raw.split_once(['e', 'E']) {
        (mantissa, exponent.parse::<i32>().ok()?)
    } else {
        (raw, 0)
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<u128>().ok()?;
    let scale = exponent - i32::try_from(fraction.len()).ok()? + 6;
    let microusd = if scale >= 0 {
        value.checked_mul(10_u128.checked_pow(scale as u32)?)?
    } else {
        let divisor = 10_u128.checked_pow(scale.unsigned_abs())?;
        value.checked_add(divisor / 2)? / divisor
    };
    u64::try_from(microusd).ok()
}

pub(crate) fn claude_usage(response: &Value, fallback_model: &str) -> Option<AiUsage> {
    let usage = response.get("usage")?;
    let tokens = AiTokenUsage {
        input_tokens: token_at(usage, &["/input_tokens", "/inputTokens"]),
        cached_input_tokens: token_at(
            usage,
            &["/cache_read_input_tokens", "/cacheReadInputTokens"],
        ),
        cache_creation_input_tokens: token_at(
            usage,
            &["/cache_creation_input_tokens", "/cacheCreationInputTokens"],
        ),
        output_tokens: token_at(usage, &["/output_tokens", "/outputTokens"]),
        reasoning_tokens: token_at(usage, &["/reasoning_tokens", "/reasoningTokens"]),
    };
    let provider_cost_microusd = response
        .get("total_cost_usd")
        .or_else(|| response.get("totalCostUsd"))
        .and_then(usd_to_microusd);
    if tokens.is_empty() && provider_cost_microusd.is_none() {
        return None;
    }
    Some(AiUsage {
        provider: "anthropic".into(),
        model: response
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(fallback_model)
            .to_owned(),
        tokens,
        provider_cost_microusd,
    })
}

pub(crate) fn codex_usage(value: &Value, fallback_model: Option<&str>) -> Option<AiUsage> {
    // exec emits a flat snake_case object while app-server reports camelCase totals.
    let root = value.get("total").unwrap_or(value);
    let reported_input = token_at(
        root,
        &["/input_tokens", "/inputTokens", "/total_input_tokens"],
    );
    let cached_input_tokens = token_at(
        root,
        &[
            "/cached_input_tokens",
            "/cachedInputTokens",
            "/input_tokens_details/cached_tokens",
        ],
    );
    let reported_output = token_at(
        root,
        &["/output_tokens", "/outputTokens", "/total_output_tokens"],
    );
    let reasoning_tokens = token_at(
        root,
        &[
            "/reasoning_tokens",
            "/reasoningTokens",
            "/output_tokens_details/reasoning_tokens",
        ],
    );
    let tokens = AiTokenUsage {
        input_tokens: reported_input.saturating_sub(cached_input_tokens),
        cached_input_tokens,
        cache_creation_input_tokens: 0,
        output_tokens: reported_output.saturating_sub(reasoning_tokens),
        reasoning_tokens,
    };
    if tokens.is_empty() {
        return None;
    }
    Some(AiUsage {
        provider: "openai".into(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .or(fallback_model)
            .unwrap_or("unknown")
            .to_owned(),
        tokens,
        provider_cost_microusd: None,
    })
}

pub(crate) fn emit(sink: Option<&Arc<dyn ProviderEventSink>>, usage: Option<AiUsage>) {
    if let (Some(sink), Some(usage)) = (sink, usage) {
        sink.emit(ProviderExecutionEvent::AiUsage { usage });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::json;

    #[test]
    fn normalizes_claude_tokens_and_exact_cost() {
        let usage = claude_usage(
            &json!({
                "model": "claude-test",
                "usage": {
                    "input_tokens": 10,
                    "cache_read_input_tokens": 3,
                    "cache_creation_input_tokens": 2,
                    "output_tokens": 4
                },
                "total_cost_usd": 0.012345
            }),
            "fallback",
        )
        .unwrap();
        assert_eq!(usage.model, "claude-test");
        assert_eq!(usage.tokens.total_tokens(), 19);
        assert_eq!(usage.provider_cost_microusd, Some(12_345));
    }

    #[test]
    fn normalizes_flat_and_nested_codex_usage() {
        let flat = codex_usage(
            &json!({"input_tokens": 4, "output_tokens": 2}),
            Some("gpt-test"),
        )
        .unwrap();
        assert_eq!(flat.tokens.input_tokens, 4);
        assert_eq!(flat.model, "gpt-test");

        let nested = codex_usage(
            &json!({"total":{"inputTokens":5,"cachedInputTokens":2,"outputTokens":3}}),
            None,
        )
        .unwrap();
        assert_eq!(nested.tokens.cached_input_tokens, 2);
    }

    #[test]
    fn missing_or_malformed_usage_remains_unaccounted() {
        assert!(claude_usage(&json!({"result":"ok"}), "fallback").is_none());
        assert!(
            claude_usage(
                &json!({"usage":{"input_tokens":"many"},"total_cost_usd":"unknown"}),
                "fallback"
            )
            .is_none()
        );
        assert!(codex_usage(&json!({"total":{"inputTokens":"many"}}), None).is_none());
    }

    #[test]
    fn claude_cost_without_tokens_is_still_accounted() {
        let usage = claude_usage(
            &json!({"usage":{},"total_cost_usd":0.0000015}),
            "claude-test",
        )
        .unwrap();
        assert_eq!(usage.provider_cost_microusd, Some(2));
        assert!(usage.tokens.is_empty());
    }
}
