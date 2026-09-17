#[allow(unused_imports)]
use super::*;

pub(super) struct ExitingFactory {
    pub(super) tracker: StateTracker,
    pub(super) spawns: AtomicUsize,
}

#[async_trait]
impl RuntimeFactory for ExitingFactory {
    async fn spawn(
        &self,
        kind: ReplicaKind,
        node_id: String,
        _spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> Result<JoinHandle<()>, SendableError> {
        self.tracker.starting(&node_id, kind.as_str());
        self.tracker.running(&node_id);
        let attempt = self.spawns.fetch_add(1, Ordering::SeqCst);
        Ok(tokio::spawn(async move {
            if attempt > 0 {
                shutdown.notified().await;
            }
        }))
    }
}
