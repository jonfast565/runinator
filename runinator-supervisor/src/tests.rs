use std::collections::BTreeMap;

use crate::config::ProcessConfig;
use crate::control::{ControlCommand, drain, enqueue};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "runinator-supervisor-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn enqueue_then_drain_round_trips_in_order() {
    let dir = temp_dir("order");

    let mut env = BTreeMap::new();
    env.insert("KEY".to_string(), "VALUE".to_string());
    let process = ProcessConfig {
        name: "worker-1".to_string(),
        command: "./worker".to_string(),
        args: vec!["--flag".to_string()],
        cwd: None,
        env,
        autostart: true,
        restart_on_failure: true,
        max_restarts_per_minute: 10,
    };

    enqueue(&dir, &ControlCommand::AddProcess { process }).unwrap();
    enqueue(
        &dir,
        &ControlCommand::StartProcess {
            name: "worker-1".to_string(),
        },
    )
    .unwrap();
    enqueue(
        &dir,
        &ControlCommand::RemoveProcess {
            name: "worker-1".to_string(),
        },
    )
    .unwrap();

    let drained = drain(&dir);
    assert_eq!(drained.len(), 3);
    assert!(matches!(drained[0], ControlCommand::AddProcess { .. }));
    assert!(matches!(drained[1], ControlCommand::StartProcess { .. }));
    assert!(matches!(drained[2], ControlCommand::RemoveProcess { .. }));

    // draining a second time yields nothing because the queue was consumed.
    assert!(drain(&dir).is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drain_skips_malformed_files() {
    let dir = temp_dir("malformed");
    std::fs::write(dir.join("00000.json"), b"not json").unwrap();
    enqueue(
        &dir,
        &ControlCommand::StopProcess {
            name: "worker-2".to_string(),
        },
    )
    .unwrap();

    let drained = drain(&dir);
    assert_eq!(drained.len(), 1);
    assert!(matches!(drained[0], ControlCommand::StopProcess { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}
