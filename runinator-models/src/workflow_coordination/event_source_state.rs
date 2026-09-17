#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSourceState {
    pub event_type: String,
    pub events_processed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_events: Option<i64>,
}
