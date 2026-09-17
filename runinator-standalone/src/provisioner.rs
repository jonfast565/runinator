use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use runinator_models::{
    errors::SendableError,
    provisioning::{NodeSpec, ProvisionBackend, ProvisionedGroup},
    replicas::ReplicaKind,
};
use runinator_provisioner::Provisioner;
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::state::StateTracker;

#[cfg(test)]
#[path = "provisioner_tests.rs"]
mod tests;

mod runtime_factory;
pub use runtime_factory::RuntimeFactory;

mod node;
use node::Node;

mod standalone_provisioner;
pub use standalone_provisioner::StandaloneProvisioner;
