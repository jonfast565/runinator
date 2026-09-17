#[allow(unused_imports)]
use super::*;

/// an event delivered to a parked `event_source` node. `type` selects which subscriptions match;
/// the rest of the body is the payload the node's filter and body see.
#[derive(Debug, Deserialize)]
pub struct EventDeliveryRequest {
    #[serde(rename = "type", default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub data: Value,
}

impl Validate for EventDeliveryRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("type", self.event_type.as_deref(), SHORT_TEXT_MAX)
    }
}
