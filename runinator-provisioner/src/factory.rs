use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use runinator_models::replicas::ReplicaKind;

use crate::supervisor::{SupervisorNodeTemplate, SupervisorProvisioner};
use crate::traits::{Provisioner, ProvisionerRegistry};

/// supervisor-backend configuration: where to read state / enqueue control, plus a spawn template
/// per node kind. keying by kind means a new kind is manageable as soon as a template is added.
#[derive(Debug, Clone)]
pub struct SupervisorBackendConfig {
    pub control_dir: PathBuf,
    pub state_file: PathBuf,
    pub templates: BTreeMap<ReplicaKind, SupervisorNodeTemplate>,
}

/// kubernetes-backend configuration: namespace and the workload (deployment or stateful set)
/// backing each node kind. keying by kind means a new kind is manageable once a workload is added.
#[derive(Debug, Clone)]
pub struct KubernetesBackendConfig {
    pub namespace: String,
    pub deployments: BTreeMap<ReplicaKind, String>,
    pub stateful_sets: BTreeMap<ReplicaKind, String>,
    pub postgres_scale_out_enabled: bool,
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
        for (kind, template) in supervisor.templates {
            backend = backend.with_template(kind, template);
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
    for (kind, name) in config.deployments {
        backend = backend.with_deployment(kind, name);
    }
    for (kind, name) in config.stateful_sets {
        backend = backend.with_stateful_set(kind, name);
    }
    if config.postgres_scale_out_enabled {
        backend = backend.with_postgres_scale_out_enabled();
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
