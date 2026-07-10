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

fn webservice_template() -> SupervisorNodeTemplate {
    SupervisorNodeTemplate {
        command: "./ws".into(),
        args: vec!["--port".into(), "8081".into()],
        env: BTreeMap::new(),
        cwd: None,
    }
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

#[tokio::test]
async fn org_group_scales_an_independent_labeled_pool() {
    let dir = temp_dir("group");
    // an existing default-pool worker must not be counted against the org's own group.
    write_snapshot(
        &dir.join("state.json"),
        &worker_snapshot(&[("prov-worker-a", "running")]),
    )
    .unwrap();

    let prov = provisioner(&dir);
    let mut spec = NodeSpec::default();
    spec.group = Some("org-acme-worker".into());
    spec.labels.insert("org".into(), "acme".into());
    let group = prov.scale(ReplicaKind::Worker, 2, &spec).await.unwrap();
    assert_eq!(group.name, "org-acme-worker");
    assert_eq!(group.desired, 2);

    let commands = drain(&dir.join("control"));
    let adds: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            ControlCommand::AddProcess { process } => Some(process.name.as_str()),
            _ => None,
        })
        .collect();
    // two fresh org-pool processes are added (the default-pool worker is untouched).
    assert_eq!(adds.len(), 2);
    assert!(adds
        .iter()
        .all(|name| name.starts_with("prov-org-acme-worker-")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn list_reports_every_kind_ghosting_the_unconfigured_ones() {
    let dir = temp_dir("list-all");
    write_snapshot(
        &dir.join("state.json"),
        &worker_snapshot(&[("prov-worker-a", "running")]),
    )
    .unwrap();

    // only a worker template is configured.
    let prov = provisioner(&dir);
    let groups = prov.list().await.unwrap();

    // every kind is present, in the canonical order.
    assert_eq!(groups.len(), ReplicaKind::ALL.len());
    for (group, kind) in groups.iter().zip(ReplicaKind::ALL.iter()) {
        assert_eq!(group.kind, *kind);
    }

    let worker = groups
        .iter()
        .find(|g| g.kind == ReplicaKind::Worker)
        .unwrap();
    assert!(worker.manageable, "configured kind is manageable");
    assert_eq!(worker.desired, 1);

    let archiver = groups
        .iter()
        .find(|g| g.kind == ReplicaKind::Archiver)
        .unwrap();
    assert!(!archiver.manageable, "unconfigured kind is a ghost row");
    assert_eq!(archiver.desired, 0);

    // control-plane kinds report a floor of one, others report zero.
    let webservice = groups
        .iter()
        .find(|g| g.kind == ReplicaKind::Webservice)
        .unwrap();
    assert_eq!(webservice.min_desired, 1);
    assert_eq!(worker.min_desired, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn supervisor_backend_supports_webservice_when_template_is_configured() {
    let dir = temp_dir("ws");
    write_snapshot(&dir.join("state.json"), &worker_snapshot(&[])).unwrap();

    let prov = SupervisorProvisioner::new(dir.join("control"), dir.join("state.json"))
        .with_template(ReplicaKind::Webservice, webservice_template());

    let group = prov
        .scale(ReplicaKind::Webservice, 1, &NodeSpec::default())
        .await
        .unwrap();
    assert_eq!(group.kind, ReplicaKind::Webservice);

    let commands = drain(&dir.join("control"));
    let ControlCommand::AddProcess { process } = &commands[0] else {
        panic!("expected an add-process command");
    };
    assert!(process.name.starts_with("prov-webservice-"));
    assert!(process.args.contains(&"--instance-id".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "kubernetes")]
#[test]
fn clone_group_deployment_renames_labels_and_injects_worker_labels() {
    use crate::kubernetes::clone_group_deployment;
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

    let base = Deployment {
        metadata: ObjectMeta {
            name: Some("worker".into()),
            resource_version: Some("123".into()),
            uid: Some("abc".into()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some([("app".to_string(), "worker".to_string())].into()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some([("app".to_string(), "worker".to_string())].into()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "worker".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: Some(Default::default()),
    };

    let mut spec = NodeSpec::default();
    spec.labels.insert("org".into(), "acme".into());
    let cloned = clone_group_deployment(base, "org-acme-worker", 3, &spec);

    // renamed, server-identity cleared, replicas set.
    assert_eq!(cloned.metadata.name.as_deref(), Some("org-acme-worker"));
    assert!(cloned.metadata.resource_version.is_none());
    assert!(cloned.metadata.uid.is_none());
    assert!(cloned.status.is_none());
    let dspec = cloned.spec.unwrap();
    assert_eq!(dspec.replicas, Some(3));
    // the org label lands on metadata, selector, and pod template so the deployment selects its pods.
    assert_eq!(
        cloned
            .metadata
            .labels
            .unwrap()
            .get("org")
            .map(String::as_str),
        Some("acme")
    );
    assert_eq!(
        dspec
            .selector
            .match_labels
            .unwrap()
            .get("org")
            .map(String::as_str),
        Some("acme")
    );
    let container = &dspec.template.spec.unwrap().containers[0];
    let env = container.env.as_ref().unwrap();
    let labels_env = env
        .iter()
        .find(|e| e.name == "RUNINATOR_WORKER_LABELS")
        .unwrap();
    assert_eq!(labels_env.value.as_deref(), Some("org=acme"));
}

#[cfg(feature = "kubernetes")]
#[tokio::test]
async fn kubernetes_supported_kinds_include_webservice_and_postgres_when_configured() {
    use crate::kubernetes::KubernetesProvisioner;

    let prov = KubernetesProvisioner::new("runinator".into())
        .with_deployment(ReplicaKind::Worker, "runinator-worker")
        .with_deployment(ReplicaKind::Webservice, "runinator-ws")
        .with_stateful_set(ReplicaKind::Postgres, "runinator-postgres");

    let kinds = prov.supported_kinds();
    assert!(kinds.contains(&ReplicaKind::Worker));
    assert!(kinds.contains(&ReplicaKind::Webservice));
    assert!(kinds.contains(&ReplicaKind::Postgres));
}

#[cfg(feature = "kubernetes")]
#[tokio::test]
async fn kubernetes_postgres_scale_out_requires_explicit_opt_in() {
    use crate::kubernetes::KubernetesProvisioner;

    let prov = KubernetesProvisioner::new("runinator".into())
        .with_stateful_set(ReplicaKind::Postgres, "runinator-postgres");

    let err = prov
        .scale(ReplicaKind::Postgres, 2, &NodeSpec::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("postgres scale-out requires"));
}
