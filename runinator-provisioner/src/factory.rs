use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use runinator_models::replicas::ReplicaKind;

use crate::supervisor::{SupervisorNodeTemplate, SupervisorProvisioner};
use crate::traits::{Provisioner, ProvisionerRegistry};

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

mod supervisor_backend_config;
pub use supervisor_backend_config::SupervisorBackendConfig;

mod kubernetes_backend_config;
pub use kubernetes_backend_config::KubernetesBackendConfig;

mod provisioner_config;
pub use provisioner_config::ProvisionerConfig;
