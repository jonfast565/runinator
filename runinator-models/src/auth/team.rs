#[allow(unused_imports)]
use super::*;

/// a team: a named principal that grants can target, with users as members.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Option<Uuid>,
    pub name: String,
    pub scope: crate::rbac::ScopeRef,
    pub created_at: DateTime<Utc>,
}
