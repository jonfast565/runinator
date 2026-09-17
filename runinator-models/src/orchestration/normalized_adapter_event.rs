#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedAdapterEvent {
    pub source: String,
    pub delivery_id: String,
    pub event_type: String,
    pub scope: String,
    pub correlation_key: String,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
}

impl NormalizedAdapterEvent {
    pub fn validate_identity(&self) -> Result<(), String> {
        for (name, value, limit) in [
            ("source", self.source.as_str(), INGRESS_SOURCE_LIMIT),
            (
                "delivery_id",
                self.delivery_id.as_str(),
                INGRESS_DELIVERY_ID_LIMIT,
            ),
            (
                "event_type",
                self.event_type.as_str(),
                INGRESS_EVENT_TYPE_LIMIT,
            ),
            ("scope", self.scope.as_str(), INGRESS_SCOPE_LIMIT),
            (
                "correlation_key",
                self.correlation_key.as_str(),
                INGRESS_CORRELATION_KEY_LIMIT,
            ),
        ] {
            if value.trim().is_empty() {
                return Err(format!("normalized adapter {name} must not be empty"));
            }
            if value.len() > limit || value.chars().any(char::is_control) {
                return Err(format!("normalized adapter {name} is not a valid identity"));
            }
        }
        Ok(())
    }
}
