#[allow(unused_imports)]
use super::*;

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
        format!("sha256:{}", hex::encode(Sha256::digest(payload)))
    }
}
