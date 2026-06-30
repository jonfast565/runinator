use std::collections::BTreeMap;
use std::path::PathBuf;

use runinator_models::provisioning::{NodeSpec, ProvisionBackend};
use runinator_models::replicas::ReplicaKind;
use runinator_supervisor::control::{drain, ControlCommand};
use runinator_supervisor::snapshot::{write_snapshot, ProcessSnapshot, StateSnapshot};

use crate::supervisor::{SupervisorNodeTemplate, SupervisorProvisioner};
use crate::traits::Provisioner;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "runinator-provisioner-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn worker_snapshot(names: &[(&str, &str)]) -> StateSnapshot {
    StateSnapshot {
        supervisor_pid: 1,
        config_path: "cfg".into(),
        started_at: "now".into(),
        updated_at: "now".into(),
        processes: names
            .iter()
            .map(|(name, status)| ProcessSnapshot {
                name: name.to_string(),
                status: status.to_string(),
                pid: None,
                restarts: 0,
                uptime_seconds: None,
                last_exit_code: None,
                last_error: None,
                started_at: None,
                command: "cmd".into(),
                cwd: "cwd".into(),
                log_file: "log".into(),
            })
            .collect(),
    }
}

fn provisioner(dir: &PathBuf) -> SupervisorProvisioner {
    let control_dir = dir.join("control");
    let state_file = dir.join("state.json");
    SupervisorProvisioner::new(control_dir, state_file).with_template(
        ReplicaKind::Worker,
        SupervisorNodeTemplate {
            command: "./worker".into(),
            args: vec!["--broker-backend".into(), "tcp".into()],
            env: BTreeMap::new(),
            cwd: None,
        },
    )
}

#[tokio::test]
async fn scale_up_enqueues_add_for_each_missing_node() {
    let dir = temp_dir("up");
    write_snapshot(
        &dir.join("state.json"),
        &worker_snapshot(&[("prov-worker-a", "running")]),
    )
    .unwrap();

    let prov = provisioner(&dir);
    let group = prov
        .scale(ReplicaKind::Worker, 3, &NodeSpec::default())
        .await
        .unwrap();
    assert_eq!(group.desired, 3);
    assert_eq!(group.backend, ProvisionBackend::Supervisor);

    let commands = drain(&dir.join("control"));
    let adds = commands
        .iter()
        .filter(|c| matches!(c, ControlCommand::AddProcess { .. }))
        .count();
    assert_eq!(adds, 2, "should add the two missing workers");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scale_down_removes_extra_nodes() {
    let dir = temp_dir("down");
    write_snapshot(
        &dir.join("state.json"),
        &worker_snapshot(&[
            ("prov-worker-a", "running"),
            ("prov-worker-b", "running"),
            ("prov-worker-c", "running"),
        ]),
    )
    .unwrap();

    let prov = provisioner(&dir);
    prov.scale(ReplicaKind::Worker, 1, &NodeSpec::default())
        .await
        .unwrap();

    let commands = drain(&dir.join("control"));
    let removes: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            ControlCommand::RemoveProcess { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(removes.len(), 2);
    // newest (highest-sorted) names are removed first.
    assert!(removes.contains(&"prov-worker-c"));
    assert!(removes.contains(&"prov-worker-b"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn add_process_carries_generated_worker_id_and_labels() {
    let dir = temp_dir("args");
    write_snapshot(&dir.join("state.json"), &worker_snapshot(&[])).unwrap();

    let prov = provisioner(&dir);
    let mut spec = NodeSpec::default();
    spec.labels.insert("pool".into(), "gpu".into());
    prov.scale(ReplicaKind::Worker, 1, &spec).await.unwrap();

    let commands = drain(&dir.join("control"));
    let ControlCommand::AddProcess { process } = &commands[0] else {
        panic!("expected an add-process command");
    };
    assert!(process.name.starts_with("prov-worker-"));
    assert_eq!(process.name, process.args[process.args.len() - 1]);
    assert!(process.args.contains(&"--worker-id".to_string()));
    assert_eq!(
        process.env.get("RUNINATOR_WORKER_LABELS"),
        Some(&"pool=gpu".to_string())
    );
    let _ = std::fs::remove_dir_all(&dir);
}
