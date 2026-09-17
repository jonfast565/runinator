#[allow(unused_imports)]
use super::*;

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
