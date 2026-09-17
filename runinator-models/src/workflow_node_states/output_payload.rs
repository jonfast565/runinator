#[allow(unused_imports)]
use super::*;

/// output node output recorded when an output node publishes its payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Value>,
}
