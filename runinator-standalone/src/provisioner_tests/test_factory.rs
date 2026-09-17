#[allow(unused_imports)]
use super::*;

pub(super) struct TestFactory {
    pub(super) tracker: StateTracker,
}

#[async_trait]
impl RuntimeFactory for TestFactory {
    async fn spawn(
        &self,
        kind: ReplicaKind,
        node_id: String,
        _spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> Result<JoinHandle<()>, SendableError> {
        self.tracker.starting(&node_id, kind.as_str());
        self.tracker.running(&node_id);
        Ok(tokio::spawn(async move {
            shutdown.notified().await;
        }))
    }
}
