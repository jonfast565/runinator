#[allow(unused_imports)]
use super::*;

/// metadata describing one configured backend and the kinds it can provision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBackendInfo {
    pub backend: ProvisionBackend,
    pub kinds: Vec<ReplicaKind>,
    /// whether the backend is reachable/usable right now.
    pub available: bool,
}
