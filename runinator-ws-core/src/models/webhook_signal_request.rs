#[allow(unused_imports)]
use super::*;

/// inbound webhook that routes a signal to a parked node by business correlation key (e.g. a ticket
/// key or PR number) rather than a run id, so external systems need not track run ids.
#[derive(Debug, Deserialize)]
pub struct WebhookSignalRequest {
    pub name: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
}

impl Validate for WebhookSignalRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("name", &self.name)?;
        required_text("correlation_key", &self.correlation_key, SHORT_TEXT_MAX)
    }
}
