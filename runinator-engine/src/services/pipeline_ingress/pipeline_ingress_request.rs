#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct PipelineIngressRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    pub provenance: Value,
    pub occurred_at: Option<chrono::DateTime<Utc>>,
}

impl PipelineIngressRequest {
    /// Reject an identity no backend can store before it reaches an insert. `scope` and
    /// `correlation_key` share one exact unique key that mysql caps at 3072 utf8mb4 bytes, so an
    /// oversized value is a dialect-dependent insert failure rather than a clean rejection.
    /// `source` here is the composed `adapter:<id>:<source>` form, which is why its bound is wider
    /// than the adapter-side one.
    pub(super) fn validate_identity(&self) -> Result<(), String> {
        for (name, value, limit) in [
            ("source", self.source.as_str(), 191usize),
            (
                "event_id",
                self.event_id.as_str(),
                INGRESS_DELIVERY_ID_LIMIT,
            ),
            (
                "event_type",
                self.event_type.as_str(),
                INGRESS_EVENT_TYPE_LIMIT,
            ),
            (
                "correlation_key",
                self.correlation_key.as_str(),
                INGRESS_CORRELATION_KEY_LIMIT,
            ),
        ] {
            if value.trim().is_empty() {
                return Err(format!("ingress {name} must not be empty"));
            }
            if value.len() > limit {
                return Err(format!(
                    "ingress {name} is longer than the {limit} characters every backend can store"
                ));
            }
        }
        Ok(())
    }
}
