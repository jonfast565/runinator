#[allow(unused_imports)]
use super::*;

/// one entry of `state.event_sources`: an inbound event parked for an event_source node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventSourceEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_event: Option<Value>,
}
