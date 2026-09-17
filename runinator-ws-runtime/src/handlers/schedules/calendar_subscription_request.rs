#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct CalendarSubscriptionRequest {
    pub scope: CalendarScope,
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

impl Validate for CalendarSubscriptionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
