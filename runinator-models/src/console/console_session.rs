#[allow(unused_imports)]
use super::*;

/// one notebook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleSession {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
