#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ExecutionProfileBundleEntry {
    pub configuration: ExecutionProfilePutRequest,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}
