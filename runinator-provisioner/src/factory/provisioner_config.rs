#[allow(unused_imports)]
use super::*;

/// the set of backends to construct; either may be absent.
#[derive(Debug, Clone, Default)]
pub struct ProvisionerConfig {
    pub supervisor: Option<SupervisorBackendConfig>,
    pub kubernetes: Option<KubernetesBackendConfig>,
}
