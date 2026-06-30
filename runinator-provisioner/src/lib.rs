//! pluggable node provisioning: a `Provisioner` trait with supervisor (local process) and
//! kubernetes (deployment scale) backends, plus a registry that ws/ctl drive over the api.

pub mod errors;
pub mod factory;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
pub mod supervisor;
pub mod traits;

pub use factory::{
    build_registry, KubernetesBackendConfig, ProvisionerConfig, SupervisorBackendConfig,
};
pub use supervisor::{SupervisorNodeTemplate, SupervisorProvisioner};
pub use traits::{Provisioner, ProvisionerRegistry};

#[cfg(test)]
mod tests;
