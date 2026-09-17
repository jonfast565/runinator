#[allow(unused_imports)]
use super::*;

/// a revocable refresh session backing a logged-in user.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub refresh_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
    /// Number of successful refresh rotations consumed by this login session.
    pub refresh_count: i64,
    /// The original login time, preserved across refresh-token rotation.
    pub created_at: DateTime<Utc>,
    /// Coarse-grained activity timestamp, updated at most once every few minutes.
    pub last_seen_at: DateTime<Utc>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}
