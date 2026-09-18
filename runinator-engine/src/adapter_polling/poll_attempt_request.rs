#[allow(unused_imports)]
use super::*;

pub struct PollAttemptRequest<'a> {
    pub adapter: &'a runinator_models::orchestration::AdapterDefinition,
    pub revision: &'a runinator_models::orchestration::AdapterRevision,
    pub request: AdapterPollRequest,
    pub claim_owner: String,
    pub dry_run: bool,
    /// when the attempt stops being answerable. it has to land at or before the claim it was taken
    /// under, or the poll row becomes re-claimable while the attempt is still outstanding and the
    /// expiry sweep looks for an owner that has already been replaced.
    pub deadline_at: chrono::DateTime<chrono::Utc>,
}
