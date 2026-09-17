#[allow(unused_imports)]
use super::*;

/// Safe result metadata for a profile definition reconciled during pack import. Publication
/// revisions, publisher identities, archive digests, and archive contents are intentionally absent.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ExecutionProfileImportResult {
    pub id: uuid::Uuid,
    pub org_id: Option<uuid::Uuid>,
    pub configuration: ExecutionProfilePutRequest,
    pub config_version: i64,
    pub updated_at: DateTime<Utc>,
}

impl From<ExecutionProfile> for ExecutionProfileImportResult {
    fn from(profile: ExecutionProfile) -> Self {
        Self {
            id: profile.id,
            org_id: profile.org_id,
            configuration: ExecutionProfilePutRequest {
                name: profile.name,
                description: profile.description,
                credential_scopes: profile.credential_scopes,
                collection: profile.collection,
                exposure: profile.exposure,
                enabled: profile.enabled,
            },
            config_version: profile.config_version,
            updated_at: profile.updated_at,
        }
    }
}
