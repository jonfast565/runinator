#[allow(unused_imports)]
use super::*;

/// process template used to spawn one node of a kind via the supervisor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SupervisorNodeTemplate {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
}
