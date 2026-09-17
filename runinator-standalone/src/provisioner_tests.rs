use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[path = "provisioner_tests/test_factory.rs"]
mod test_factory;
use test_factory::TestFactory;

#[path = "provisioner_tests/exiting_factory.rs"]
mod exiting_factory;
use exiting_factory::ExitingFactory;
