#[allow(unused_imports)]
use super::*;

/// one name in a session's scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleBinding {
    pub id: Uuid,
    pub session_id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<Uuid>,
    pub value: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
