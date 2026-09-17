#[allow(unused_imports)]
use super::*;

/// one bucket in a `ListBuckets` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketSummary {
    pub name: String,
    pub created_at: DateTime<Utc>,
}
