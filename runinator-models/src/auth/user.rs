#[allow(unused_imports)]
use super::*;

/// a user account in wire form. never carries a password hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Option<Uuid>,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
