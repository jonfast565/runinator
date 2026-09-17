#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayOptions {
    #[serde(default)]
    pub from_step_id: Option<String>,
    #[serde(default)]
    pub plan_fingerprint: Option<String>,
    #[serde(default)]
    pub acknowledge_review: bool,
}
