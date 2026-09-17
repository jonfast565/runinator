#[allow(unused_imports)]
use super::*;

#[async_trait]
pub trait RuntimeFactory: Send + Sync {
    async fn spawn(
        &self,
        kind: ReplicaKind,
        node_id: String,
        spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> Result<JoinHandle<()>, SendableError>;
}
