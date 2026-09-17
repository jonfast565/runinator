#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct OrchestrationBindingUpdate {
    pub expected_version: i64,
    pub status: OrchestrationStatus,
    pub current_phase: Option<String>,
    pub current_attempt: i64,
    pub current_epoch: i64,
    pub restart_member: Option<String>,
    pub resume_existing_epoch: bool,
    pub subject_revision: Option<String>,
    pub resources: Value,
    pub budgets: BTreeMap<String, u32>,
    pub last_reduced_sequence: i64,
    pub finished_at: Option<DateTime<Utc>>,
}
