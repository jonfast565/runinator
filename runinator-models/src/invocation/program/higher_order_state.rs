#[allow(unused_imports)]
use super::*;

/// Serializable state for a higher-order invocation while one of its lambda calls is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HigherOrderState {
    pub name: String,
    pub closure: usize,
    pub items: Vec<Value>,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accumulator: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyed: Vec<(Value, Value)>,
}
