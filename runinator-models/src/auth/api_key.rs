#[allow(unused_imports)]
use super::*;

/// API key metadata in wire form. never carries the secret or its hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_role: Option<SystemRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    pub key_prefix: String,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
}
