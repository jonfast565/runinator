#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimReleaseRequest {
    pub scheduler_id: String,
}

impl Validate for SchedulerRunClaimReleaseRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scheduler_id", &self.scheduler_id)
    }
}
