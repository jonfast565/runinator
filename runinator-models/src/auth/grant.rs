#[allow(unused_imports)]
use super::*;

/// a single access grant on a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: Option<Uuid>,
    pub resource_type: ResourceType,
    pub resource_id: Uuid,
    pub principal_type: PrincipalType,
    pub principal_id: Uuid,
    pub permission: Permission,
    pub created_at: DateTime<Utc>,
}
