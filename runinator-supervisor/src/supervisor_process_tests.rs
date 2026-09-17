//! restart budgets, spawn failures, and forced shutdown through fake processes.
use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

fn process() -> ManagedProcess {
    let dir = std::env::temp_dir().join(format!(
        "runinator-supervisor-traits-{}-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap(),
        NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let paths = Paths {
        config_path: dir.clone(),
        config_dir: dir.clone(),
        state_dir: dir.clone(),
        pid_file: dir.clone(),
        stop_file: dir.clone(),
        state_file: dir.clone(),
        control_dir: dir.clone(),
        logs_dir: dir.clone(),
        supervisor_log: dir,
    };
    let config = serde_json::from_value(
        serde_json::json!({"name":"fake", "command":"missing", "max_restarts_per_minute":1}),
    )
    .unwrap();
    let mut process = build_one_process(&config, &paths);
    process.backend = Arc::new(FailingBackend);
    process
}
#[test]
fn spawn_failure_exhausts_restart_budget() {
    let mut process = process();
    attempt_start(&mut process, Duration::ZERO).unwrap();
    assert!(matches!(process.status, ProcStatus::Backoff));
    poll_process(&mut process, Instant::now(), Duration::ZERO).unwrap();
    assert!(matches!(process.status, ProcStatus::Failed));
    assert!(process.next_restart_at.is_none());
    assert_eq!(process.restarts, 1);
    fs::remove_dir_all(process.logs_dir).unwrap();
}
#[test]
fn shutdown_escalates_and_reaps() {
    let mut process = process();
    let calls = Arc::new(Mutex::new(Vec::new()));
    process.child = Some(Box::new(FakeChild(calls.clone())));
    stop_children(std::slice::from_mut(&mut process), Duration::ZERO).unwrap();
    assert_eq!(*calls.lock().unwrap(), ["terminate", "kill", "wait"]);
    assert!(process.child.is_none());
    assert!(matches!(process.status, ProcStatus::Stopped));
    fs::remove_dir_all(process.logs_dir).unwrap();
}

#[path = "supervisor_process_tests/failing_backend.rs"]
mod failing_backend;
use failing_backend::FailingBackend;

#[path = "supervisor_process_tests/fake_child.rs"]
mod fake_child;
use fake_child::FakeChild;
