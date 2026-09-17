#[allow(unused_imports)]
use super::*;

/// administrative view of a scoped, single-use agent enrollment token. the secret is never
/// returned after creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnrollmentToken {
    pub token_id: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub service_url: String,
    #[serde(default)]
    pub spki_pin: Option<String>,
    /// When true, redemption creates a non-expiring machine credential. Otherwise the issued
    /// credential expires with this enrollment grant.
    #[serde(default)]
    pub permanent: bool,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub consumed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub issued_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
