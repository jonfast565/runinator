#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfilePublishRequest {
    pub digest: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}
