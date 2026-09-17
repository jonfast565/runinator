#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOwnership {
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    pub tenant: ScopeRef,
    pub owner: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub authz_version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
