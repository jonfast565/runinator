use std::path::PathBuf;
use std::sync::Arc;

use runinator_models::replicas::ReplicaKind;

use crate::supervisor::{SupervisorNodeTemplate, SupervisorProvisioner};
use crate::traits::{Provisioner, ProvisionerRegistry};

/// supervisor-backend configuration: where to read state / enqueue control, plus spawn templates.
#[derive(Debug, Clone)]
pub struct SupervisorBackendConfig {
    pub control_dir: PathBuf,
    pub state_file: PathBuf,
    pub worker_template: Option<SupervisorNodeTemplate>,
    pub waker_template: Option<SupervisorNodeTemplate>,
}

/// kubernetes-backend configuration: namespace and the Deployment backing each node kind.
#[derive(Debug, Clone)]
pub struct KubernetesBackendConfig {
    pub namespace: String,
    pub worker_deployment: Option<String>,
    pub waker_deployment: Option<String>,
}

/// the set of backends to construct; either may be absent.
#[derive(Debug, Clone, Default)]
pub struct ProvisionerConfig {
    pub supervisor: Option<SupervisorBackendConfig>,
    pub kubernetes: Option<KubernetesBackendConfig>,
}

/// builds a registry from config. backends without any template/deployment are still registered so
/// they can report their (empty) supported-kinds set; an all-absent config yields an empty registry.
pub fn build_registry(config: ProvisionerConfig) -> ProvisionerRegistry {
    let mut provisioners: Vec<Arc<dyn Provisioner>> = Vec::new();

    if let Some(supervisor) = config.supervisor {
        let mut backend = SupervisorProvisioner::new(supervisor.control_dir, supervisor.state_file);
        if let Some(template) = supervisor.worker_template {
            backend = backend.with_template(ReplicaKind::Worker, template);
        }
        if let Some(template) = supervisor.waker_template {
            backend = backend.with_template(ReplicaKind::Waker, template);
        }
        provisioners.push(Arc::new(backend));
    }

    if let Some(kubernetes) = config.kubernetes {
        build_kubernetes(&mut provisioners, kubernetes);
    }

    ProvisionerRegistry::new(provisioners)
}

#[cfg(feature = "kubernetes")]
fn build_kubernetes(provisioners: &mut Vec<Arc<dyn Provisioner>>, config: KubernetesBackendConfig) {
    use crate::kubernetes::KubernetesProvisioner;

    let mut backend = KubernetesProvisioner::new(config.namespace);
    if let Some(name) = config.worker_deployment {
        backend = backend.with_deployment(ReplicaKind::Worker, name);
    }
    if let Some(name) = config.waker_deployment {
        backend = backend.with_deployment(ReplicaKind::Waker, name);
    }
    provisioners.push(Arc::new(backend));
}

#[cfg(not(feature = "kubernetes"))]
fn build_kubernetes(
    _provisioners: &mut Vec<Arc<dyn Provisioner>>,
    _config: KubernetesBackendConfig,
) {
    log::warn!(
        "kubernetes provisioner configured but runinator-provisioner was built without the 'kubernetes' feature"
    );
}
