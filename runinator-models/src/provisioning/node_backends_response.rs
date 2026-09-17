#[allow(unused_imports)]
use super::*;

/// response listing every configured provisioning backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBackendsResponse {
    pub backends: Vec<NodeBackendInfo>,
}
