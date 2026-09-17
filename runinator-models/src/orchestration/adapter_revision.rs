#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRevision {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub revision: i64,
    pub kind_version: String,
    /// SHA-256 of the adapter-kind metadata used to validate this immutable revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_digest: Option<String>,
    /// Selects the external delivery mechanism for this immutable revision. Missing values in
    /// older persisted revisions are intentionally interpreted as `webhook`.
    #[serde(default)]
    pub transport: AdapterTransport,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub authentication: AdapterAuthentication,
    #[serde(default)]
    pub identity_configuration: Value,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
}
