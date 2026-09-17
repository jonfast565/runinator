#[allow(unused_imports)]
use super::*;

/// Immutable envelope submitted to a pipeline's ingress policy. Keeping the event identity,
/// correlation, payload, and provenance together prevents ingress callers from accidentally
/// mismatching the durable audit fields.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineIngressRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    pub provenance: Value,
}
