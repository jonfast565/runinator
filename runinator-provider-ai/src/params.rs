use std::collections::HashMap;

use runinator_models::value::Value;
use serde::Deserialize;

pub(crate) fn default_binary() -> String {
    "claude".into()
}

pub(crate) fn default_model() -> String {
    "claude-opus-5".into()
}

pub(crate) fn default_output_format() -> String {
    "json".into()
}

pub(crate) fn default_codex_binary() -> String {
    "codex".into()
}

pub(crate) fn default_codex_sandbox() -> String {
    "read_only".into()
}

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);

mod ai_command_params;
pub(crate) use ai_command_params::AiCommandParams;

mod claude_code_params;
pub(crate) use claude_code_params::ClaudeCodeParams;

mod codex_params;
pub(crate) use codex_params::CodexParams;
