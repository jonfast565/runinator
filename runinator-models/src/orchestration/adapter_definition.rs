#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDefinition {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub kind: String,
    pub current_revision: i64,
    pub enabled: bool,
    pub endpoint_identity: String,
    pub has_admitted_binding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
