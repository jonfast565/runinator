#[allow(unused_imports)]
use super::*;

/// kubernetes-backend configuration: namespace and the workload (deployment or stateful set)
/// backing each node kind. keying by kind means a new kind is manageable once a workload is added.
#[derive(Debug, Clone)]
pub struct KubernetesBackendConfig {
    pub namespace: String,
    pub deployments: BTreeMap<ReplicaKind, String>,
    pub stateful_sets: BTreeMap<ReplicaKind, String>,
    pub postgres_scale_out_enabled: bool,
}
