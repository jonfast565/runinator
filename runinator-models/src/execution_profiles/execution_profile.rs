#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfile {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    pub collection: ExecutionProfileCollectionSpec,
    pub exposure: ExecutionProfileExposureSpec,
    pub config_version: i64,
    pub config_digest: String,
    pub enabled: bool,
    pub current_revision: Option<i64>,
    pub current_digest: Option<String>,
    pub current_publisher_id: Option<Uuid>,
    pub published_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub refresh_requested_at: Option<DateTime<Utc>>,
    pub health: ExecutionProfileHealth,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
