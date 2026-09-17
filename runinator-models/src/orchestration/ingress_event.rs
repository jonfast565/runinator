#[allow(unused_imports)]
use super::*;

/// Opaque event accepted by the generic workflow/pipeline ingress surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressEvent {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
}
