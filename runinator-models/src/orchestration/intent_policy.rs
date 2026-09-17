#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentPolicy {
    pub effect: ControlEffect,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce_seconds: Option<u64>,
    #[serde(default)]
    pub stop: EpochStopAction,
    #[serde(default)]
    pub restart: RestartSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision_pointer: Option<String>,
    #[serde(default)]
    pub allow_self_originated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_name: Option<String>,
}
