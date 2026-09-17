#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewCalendarSubscriptionRecord {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub scope: ScopeRef,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
}
