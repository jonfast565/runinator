#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDispatchClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub limit: Option<i64>,
}

impl Validate for ActionDispatchClaimRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scheduler_id", &self.scheduler_id)?;
        positive_limit("limit", self.limit, 1000)
    }
}
