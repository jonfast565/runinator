#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationIntentWake {
    pub binding_id: Uuid,
    pub intent: String,
}
