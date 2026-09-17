#[allow(unused_imports)]
use super::*;

pub struct PollAttemptRequest<'a> {
    pub adapter: &'a runinator_models::orchestration::AdapterDefinition,
    pub revision: &'a runinator_models::orchestration::AdapterRevision,
    pub request: AdapterPollRequest,
    pub claim_owner: String,
    pub dry_run: bool,
}
