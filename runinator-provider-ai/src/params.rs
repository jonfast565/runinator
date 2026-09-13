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
    pub interactive: bool,
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
    /// Keep Claude Code alive as a JSONL session so a mission controller can steer it between
    /// turns. This is intentionally distinct from `interactive`, which owns a human-facing PTY.
    #[serde(default)]
    pub harnessed: bool,
    /// A descriptive role retained in the action result and streamed progress records.
    #[serde(default)]
    pub role: Option<String>,
    /// Resume an explicit Claude Code session. Omission always starts a fresh session.
    #[serde(default)]
    pub resume_session: Option<String>,
    /// An explicit Claude Code MCP configuration file made available to this session.
    #[serde(default)]
    pub mcp_config: Option<String>,
    /// Inject Runinator's fixed capability-reduced mission MCP server. Unlike `mcp_config`, this
    /// does not accept caller-authored commands or paths.
    #[serde(default)]
    pub mission_mcp: bool,
    /// Mission binding UUID enforced by the capability-reduced MCP subprocess.
    #[serde(default)]
    pub mission_id: Option<String>,
    /// Bound autonomous work performed in one Claude Code turn.
    #[serde(default)]
    pub max_turns: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct CodexParams {
    #[serde(default = "default_codex_binary")]
    pub binary: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub harnessed: bool,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub resume_thread: Option<String>,
    #[serde(default)]
    pub session_slot: Option<String>,
    #[serde(default)]
    pub mission_mcp: bool,
    #[serde(default)]
    pub mission_id: Option<String>,
    #[serde(default = "default_codex_sandbox")]
    pub sandbox: String,
    #[serde(default)]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

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
