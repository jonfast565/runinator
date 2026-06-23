use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use super::{spawn_liveness, touch_liveness};

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("runinator-liveness-test-{}-{}", name, uuid_like()));
    path
}

// small unique suffix without pulling in the uuid crate for tests.
fn uuid_like() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn touch_liveness_writes_an_empty_file() {
    let path = temp_path("touch");
    assert!(!path.exists());
    touch_liveness(&path).unwrap();
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap(), Vec::<u8>::new());
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn spawn_liveness_returns_none_for_blank_path() {
    let shutdown = Arc::new(Notify::new());
    assert!(spawn_liveness("", DEFAULT_INTERVAL, shutdown.clone()).is_none());
    assert!(spawn_liveness("   ", DEFAULT_INTERVAL, shutdown).is_none());
}

#[tokio::test]
async fn spawn_liveness_writes_the_file_and_stops_on_shutdown() {
    let path = temp_path("spawn");
    let path_str = path.to_string_lossy().to_string();
    let shutdown = Arc::new(Notify::new());

    let handle = spawn_liveness(&path_str, DEFAULT_INTERVAL, shutdown.clone())
        .expect("non-blank path should spawn a task");

    // the file is written on the first loop iteration before sleeping.
    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(path.exists(), "liveness file should be created");

    shutdown.notify_waiters();
    // the task should observe the shutdown notification and return.
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("liveness task should stop after shutdown")
        .expect("liveness task should not panic");

    let _ = std::fs::remove_file(&path);
}

const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);
