//! terminal AI usage attachment semantics.

use super::effect_worker::RetainedAiUsage;
use runinator_models::ai_usage::{AiTokenUsage, AiUsage};

#[test]
fn terminal_usage_can_only_be_attached_once() {
    let retained = RetainedAiUsage::default();
    retained.retain(AiUsage {
        provider: "openai".into(),
        model: "gpt-test".into(),
        tokens: AiTokenUsage {
            input_tokens: 3,
            ..Default::default()
        },
        provider_cost_microusd: None,
    });

    assert_eq!(retained.take().unwrap().tokens.input_tokens, 3);
    assert!(retained.take().is_none());
}
