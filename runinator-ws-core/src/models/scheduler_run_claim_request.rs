#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub statuses: Vec<WorkflowStatus>,
    #[serde(default)]
    pub limit: Option<i64>,
}

impl Validate for SchedulerRunClaimRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scheduler_id", &self.scheduler_id)?;
        positive_limit("limit", self.limit, 1000)
    }
}
