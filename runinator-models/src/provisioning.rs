use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::replicas::ReplicaKind;

/// which provisioning backend manages a node group.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionBackend {
    Supervisor,
    Kubernetes,
}

impl ProvisionBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supervisor => "supervisor",
            Self::Kubernetes => "kubernetes",
        }
    }
}

impl TryFrom<&str> for ProvisionBackend {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "supervisor" => Ok(Self::Supervisor),
            "kubernetes" => Ok(Self::Kubernetes),
            other => Err(format!("Unknown provisioning backend '{other}'")),
        }
    }
}

/// optional knobs a caller can attach to a scale request; each backend interprets
/// what it can and ignores the rest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeSpec {
    /// routing labels to advertise on spun-up nodes (supervisor backend).
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// container image override (kubernetes backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// extra command-line args appended to spun-up nodes (supervisor backend).
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// backend-local group key that namespaces a node pool apart from the kind's default group, so
    /// e.g. per-org dedicated pools scale independently. `None` uses the kind's default group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// one provisionable node group and its current sizing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedGroup {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    /// backend-local group name (deployment name or supervised process prefix).
    pub name: String,
    /// requested node count.
    pub desired: u32,
    /// nodes currently reporting available/running.
    pub available: u32,
    /// false when the backend can observe but not change this group.
    pub manageable: bool,
}

/// one provisioned node instance within a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedNode {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub node_id: String,
    pub status: String,
}

/// metadata describing one configured backend and the kinds it can provision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBackendInfo {
    pub backend: ProvisionBackend,
    pub kinds: Vec<ReplicaKind>,
    /// whether the backend is reachable/usable right now.
    pub available: bool,
}

/// response listing every configured provisioning backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBackendsResponse {
    pub backends: Vec<NodeBackendInfo>,
}

/// set the desired node count for a kind on a backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleNodesRequest {
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub desired: u32,
    #[serde(default)]
    pub spec: NodeSpec,
}

/// stop/remove a single provisioned node instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopNodeRequest {
    pub backend: ProvisionBackend,
    pub node_id: String,
}
