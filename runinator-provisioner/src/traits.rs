use std::sync::Arc;

use async_trait::async_trait;
use runinator_models::errors::SendableError;
use runinator_models::provisioning::{
    NodeBackendInfo, NodeSpec, ProvisionBackend, ProvisionedGroup,
};
use runinator_models::replicas::ReplicaKind;

use crate::errors::UNKNOWN_BACKEND;

/// a pluggable backend that can observe and adjust the number of running nodes of a kind.
#[async_trait]
pub trait Provisioner: Send + Sync {
    /// which backend this implements.
    fn backend(&self) -> ProvisionBackend;

    /// node kinds this backend can manage.
    fn supported_kinds(&self) -> Vec<ReplicaKind>;

    /// whether the backend is reachable/usable right now.
    async fn available(&self) -> bool;

    /// the current node groups and their sizing.
    async fn list(&self) -> Result<Vec<ProvisionedGroup>, SendableError>;

    /// set the desired node count for a kind.
    async fn scale(
        &self,
        kind: ReplicaKind,
        desired: u32,
        spec: &NodeSpec,
    ) -> Result<ProvisionedGroup, SendableError>;

    /// stop/remove a single node instance.
    async fn stop(&self, node_id: &str) -> Result<(), SendableError>;
}

/// holds every configured provisioner so handlers can fan out or target one backend.
#[derive(Clone, Default)]
pub struct ProvisionerRegistry {
    provisioners: Vec<Arc<dyn Provisioner>>,
}

impl ProvisionerRegistry {
    pub fn new(provisioners: Vec<Arc<dyn Provisioner>>) -> Self {
        Self { provisioners }
    }

    pub fn is_empty(&self) -> bool {
        self.provisioners.is_empty()
    }

    /// the provisioner for a backend, if configured.
    pub fn get(&self, backend: ProvisionBackend) -> Option<Arc<dyn Provisioner>> {
        self.provisioners
            .iter()
            .find(|p| p.backend() == backend)
            .cloned()
    }

    /// resolve a backend or return the standard unknown-backend error.
    pub fn require(
        &self,
        backend: ProvisionBackend,
    ) -> Result<Arc<dyn Provisioner>, SendableError> {
        self.get(backend)
            .ok_or_else(|| UNKNOWN_BACKEND.error(backend.as_str()))
    }

    /// describe every configured backend and the kinds it supports.
    pub async fn backends(&self) -> Vec<NodeBackendInfo> {
        let mut out = Vec::with_capacity(self.provisioners.len());
        for provisioner in &self.provisioners {
            out.push(NodeBackendInfo {
                backend: provisioner.backend(),
                kinds: provisioner.supported_kinds(),
                available: provisioner.available().await,
            });
        }
        out
    }

    /// list groups across every configured backend, skipping backends that error.
    pub async fn list_all(&self) -> Vec<ProvisionedGroup> {
        let mut groups = Vec::new();
        for provisioner in &self.provisioners {
            match provisioner.list().await {
                Ok(mut found) => groups.append(&mut found),
                Err(err) => log::warn!(
                    "provisioner backend {} list failed: {err}",
                    provisioner.backend().as_str()
                ),
            }
        }
        groups
    }
}
