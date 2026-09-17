#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct SessionMetadata {
    pub(super) created_at: chrono::DateTime<Utc>,
    pub(super) user_agent: Option<String>,
    pub(super) ip_address: Option<String>,
}
