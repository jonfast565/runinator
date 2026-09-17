#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimRenewRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
}

impl Validate for SchedulerRunClaimRenewRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scheduler_id", &self.scheduler_id)
    }
}
