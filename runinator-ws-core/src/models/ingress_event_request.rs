#[allow(unused_imports)]
use super::*;

/// Opaque provider-neutral event submitted to a workflow or pipeline ingress policy.
#[derive(Debug, Deserialize)]
pub struct IngressEventRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
    #[serde(default)]
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Validate for IngressEventRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("source", &self.source)?;
        required_text("event_id", &self.event_id, SHORT_TEXT_MAX)?;
        identifier("event_type", &self.event_type)?;
        required_text("correlation_key", &self.correlation_key, SHORT_TEXT_MAX)
    }
}
