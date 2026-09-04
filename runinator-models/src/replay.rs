//! Read-only replay safety review and the acknowledgement bound to it.
use crate::{value::Value, workflows::WorkflowDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayVerdict {
    Safe,
    Review,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySeedReceipt {
    pub node_id: String,
    pub effect_id: Uuid,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayAction {
    pub node_id: String,
    pub provider: String,
    pub function: String,
    pub declared_idempotency_key: Option<Value>,
    /// Historical resolved key, not a guarantee about the next execution.
    pub previous_resolved_idempotency_keys: Vec<Value>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayPlan {
    pub source_run_id: Uuid,
    pub from_step_id: Option<String>,
    pub workflow_snapshot: Option<WorkflowDefinition>,
    pub seeded_receipts: Vec<ReplaySeedReceipt>,
    pub actions: Vec<ReplayAction>,
    pub reasons: Vec<String>,
    pub verdict: ReplayVerdict,
    pub plan_fingerprint: String,
}

impl ReplayPlan {
    pub fn fingerprint(payload: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("sha256:{:x}", Sha256::digest(payload))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayOptions {
    #[serde(default)]
    pub from_step_id: Option<String>,
    #[serde(default)]
    pub plan_fingerprint: Option<String>,
    #[serde(default)]
    pub acknowledge_review: bool,
}
