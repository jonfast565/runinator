use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::replicas::ReplicaKind;
use crate::validation::{Validate, ValidationError, identifier};

/// which provisioning backend manages a node group.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionBackend {
    Standalone,
    Supervisor,
    Kubernetes,
}

impl ProvisionBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Supervisor => "supervisor",
            Self::Kubernetes => "kubernetes",
        }
    }
}

impl TryFrom<&str> for ProvisionBackend {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "standalone" => Ok(Self::Standalone),
            "supervisor" => Ok(Self::Supervisor),
            "kubernetes" => Ok(Self::Kubernetes),
            other => Err(format!("Unknown provisioning backend '{other}'")),
        }
    }
}

mod node_spec;
pub use node_spec::NodeSpec;

mod provisioned_group;
pub use provisioned_group::ProvisionedGroup;

mod provisioned_node;
pub use provisioned_node::ProvisionedNode;

mod node_backend_info;
pub use node_backend_info::NodeBackendInfo;

mod node_backends_response;
pub use node_backends_response::NodeBackendsResponse;

mod scale_nodes_request;
pub use scale_nodes_request::ScaleNodesRequest;

mod stop_node_request;
pub use stop_node_request::StopNodeRequest;
