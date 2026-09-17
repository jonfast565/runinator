#[allow(unused_imports)]
use super::*;

#[derive(Debug, serde::Deserialize)]
pub struct AiUsageQuery {
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
}
