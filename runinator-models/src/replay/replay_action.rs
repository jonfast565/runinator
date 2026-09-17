#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayAction {
    pub node_id: String,
    pub provider: String,
    pub function: String,
    pub declared_idempotency_key: Option<Value>,
    /// Historical resolved key, not a guarantee about the next execution.
    pub previous_resolved_idempotency_keys: Vec<Value>,
    pub reason: String,
}
