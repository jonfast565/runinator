#[allow(unused_imports)]
use super::*;

/// one object in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSummary {
    pub key: String,
    pub size: u64,
    pub sha256: String,
    pub last_modified: DateTime<Utc>,
}
