#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowTryFrame {
    pub try_key: String,
    pub phase: WorkflowTryPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch: Option<usize>,
    /// Preferred catch target for a timed-out step (`on_timeout`), falling back to `catch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_timeout: Option<usize>,
    /// Preferred catch target for a rejected step (`on_reject`), falling back to `catch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_reject: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finally: Option<usize>,
    /// Captured before `finally` runs, then re-applied after it completes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_failure: Option<String>,
}
