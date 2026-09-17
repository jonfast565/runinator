#[allow(unused_imports)]
use super::*;

/// External identity explicitly linked to a Runinator human principal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIdentity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub subject: String,
    pub created_at: DateTime<Utc>,
}
