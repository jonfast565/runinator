#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct AdapterPollDispatch {
    pub dry_run: bool,
    pub deadline_at: DateTime<Utc>,
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub adapter_revision: i64,
    pub profile_id: Uuid,
    pub claim_owner: String,
    pub command: runinator_comm::EffectCommand,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
