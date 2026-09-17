#[allow(unused_imports)]
use super::*;

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
