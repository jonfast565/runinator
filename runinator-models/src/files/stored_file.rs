#[allow(unused_imports)]
use super::*;

/// Durable metadata for a file object. The opaque storage URI remains server-side and is excluded
/// from the command-center descriptor wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFile {
    pub descriptor: FileDescriptor,
    pub scope: FileScope,
    pub org_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    pub uri: String,
    pub revision: i64,
    pub current: bool,
    pub archived: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
