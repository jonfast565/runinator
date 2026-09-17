#[allow(unused_imports)]
use super::*;

/// a workflow's concurrency limit, read from `definition.metadata.concurrency`. absent metadata
/// means [`WorkflowConcurrency::unlimited`], which is the pre-policy behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowConcurrency {
    /// the number of non-terminal runs allowed at once. `0` means unlimited.
    #[serde(default)]
    pub max_concurrent_runs: i64,
    #[serde(default)]
    pub on_conflict: ConcurrencyPolicy,
}

impl Default for WorkflowConcurrency {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl WorkflowConcurrency {
    pub const fn unlimited() -> Self {
        Self {
            max_concurrent_runs: 0,
            on_conflict: ConcurrencyPolicy::Allow,
        }
    }

    /// true when this policy can ever decline a firing. an unlimited or `allow` policy never does,
    /// so the trigger loop can skip counting active runs entirely.
    pub fn is_enforced(&self) -> bool {
        self.max_concurrent_runs > 0 && self.on_conflict != ConcurrencyPolicy::Allow
    }

    /// read the policy out of a workflow graph's `metadata` object. an unparseable or missing
    /// `concurrency` entry falls back to unlimited rather than failing the firing.
    pub fn from_metadata(metadata: &Value) -> Self {
        metadata
            .get("concurrency")
            .and_then(|value| serde_json::from_value(value.clone().into()).ok())
            .unwrap_or_else(Self::unlimited)
    }
}
