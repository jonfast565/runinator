#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    pub scope: ScopeRef,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
