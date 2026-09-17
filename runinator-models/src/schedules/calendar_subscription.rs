#[allow(unused_imports)]
use super::*;

/// A revocable, purpose-specific calendar subscription. The secret is returned only at creation;
/// persistence keeps its SHA-256 hash so a database read cannot reveal a live feed URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSubscription {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub scope: ScopeRef,
    pub created_at: DateTime<Utc>,
}
