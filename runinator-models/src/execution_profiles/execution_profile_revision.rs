#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileRevision {
    pub profile_id: Uuid,
    pub revision: i64,
    pub digest: String,
    pub size_bytes: i64,
    pub publisher_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Server-side encrypted blob URI. Never serialize it onto an HTTP response.
    #[serde(skip_serializing, default)]
    pub uri: String,
}
