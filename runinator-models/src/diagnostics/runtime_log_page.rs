#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogPage {
    pub records: Vec<RuntimeLogRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub dropped: u64,
    pub retention_seconds: i64,
}
