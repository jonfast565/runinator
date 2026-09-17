#[allow(unused_imports)]
use super::*;

/// Safe, user-facing view of one refresh session. Credential material is never included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionSummary {
    pub id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}
