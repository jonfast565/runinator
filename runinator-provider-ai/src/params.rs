use std::collections::HashMap;

use runinator_models::value::Value;
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct AiCommandParams {
    pub command: String,
    pub input: Option<Value>,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeCodeParams {
    #[serde(default = "default_binary")]
    pub binary: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<String>,
    #[serde(default = "default_output_format")]
    pub output_format: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
}

pub(crate) fn default_binary() -> String {
    "claude".into()
}

pub(crate) fn default_model() -> String {
    "claude-sonnet-4-6".into()
}

pub(crate) fn default_output_format() -> String {
    "json".into()
}

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);
