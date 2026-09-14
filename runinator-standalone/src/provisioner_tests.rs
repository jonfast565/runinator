use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestFactory {
    tracker: StateTracker,
}

struct ExitingFactory {
    tracker: StateTracker,
    spawns: AtomicUsize,
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

#[tokio::test]
async fn scales_runtime_tasks_up_and_down() {
    let tracker = StateTracker::new();
    let provisioner = StandaloneProvisioner::new(
        Arc::new(TestFactory {
            tracker: tracker.clone(),
        }),
        tracker,
    );

    let scaled = provisioner
        .scale(ReplicaKind::Worker, 2, &NodeSpec::default())
        .await
        .unwrap();
    assert_eq!(scaled.desired, 2);
    assert_eq!(scaled.available, 2);

    let scaled = provisioner
        .scale(ReplicaKind::Worker, 1, &NodeSpec::default())
        .await
        .unwrap();
    assert_eq!(scaled.desired, 1);
    provisioner.shutdown_all().await;
}

#[tokio::test]
async fn replaces_a_runtime_that_exits_unexpectedly() {
    let tracker = StateTracker::new();
    let provisioner = Arc::new(StandaloneProvisioner::new(
        Arc::new(ExitingFactory {
            tracker: tracker.clone(),
            spawns: AtomicUsize::new(0),
        }),
        tracker.clone(),
    ));
    let shutdown = Arc::new(Notify::new());
    let supervisor = tokio::spawn(provisioner.clone().supervise(shutdown.clone()));

    provisioner
        .scale(ReplicaKind::Worker, 1, &NodeSpec::default())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2_200)).await;

    let component = tracker.snapshot().components.pop().unwrap();
    assert_eq!(component.status, "running");
    assert_eq!(component.restarts, 1);

    provisioner.shutdown_all().await;
    shutdown.notify_one();
    shutdown.notify_waiters();
    supervisor.await.unwrap();
}
