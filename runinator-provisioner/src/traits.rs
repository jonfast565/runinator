use std::sync::Arc;

use async_trait::async_trait;
use runinator_models::errors::SendableError;
use runinator_models::provisioning::{
    NodeBackendInfo, NodeSpec, ProvisionBackend, ProvisionedGroup,
};
use runinator_models::replicas::ReplicaKind;

use crate::errors::UNKNOWN_BACKEND;

mod provisioner;
pub use provisioner::Provisioner;

mod provisioner_registry;
pub use provisioner_registry::ProvisionerRegistry;
