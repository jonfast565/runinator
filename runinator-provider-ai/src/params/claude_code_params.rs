#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct ClaudeCodeParams {
    #[serde(default = "default_binary")]
    pub binary: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub prompt: String,
    /// Stable pack-local identity for the prompt text. The provider records it with the digest of
    /// the exact text sent to Claude and uses it to resolve execution-profile overrides.
    #[serde(default)]
    pub prompt_asset: Option<String>,
    /// Organization-scoped override resolved by the workflow from the settings store. An empty
    /// value is treated as absent so packs can ship a nullable/default slot.
    #[serde(default)]
    pub prompt_override: Option<String>,
    /// Dynamic invocation context appended after asset/override selection. Keeping context separate
    /// lets one prompt variant serve many cases without copying runtime values into the asset.
    #[serde(default)]
    pub prompt_context: Option<String>,
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
    /// JSON Schema enforced by Claude Code and verified again before the effect succeeds.
    #[serde(default)]
    pub output_schema: Option<Value>,
}
