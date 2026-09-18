//! selection of individual services for a partial deploy.
use super::*;

#[test]
fn resolves_known_services_and_preserves_order() {
    let keys = vec!["worker".to_string(), "ws".to_string()];
    let targets = DeployTarget::resolve(&keys).expect("both services are selectable");
    assert_eq!(
        targets.iter().map(|target| target.key).collect::<Vec<_>>(),
        vec!["worker", "ws"]
    );
}

#[test]
fn rejects_an_unknown_service() {
    let error = DeployTarget::resolve(&["postgres".to_string()])
        .expect_err("postgres is infrastructure, not a selectable service");
    assert!(error.to_string().contains("unknown service 'postgres'"));
}

#[test]
fn deduplicates_repeated_selections() {
    let keys = vec!["ws".to_string(), "ws".to_string()];
    let targets = DeployTarget::resolve(&keys).expect("ws is selectable");
    assert_eq!(targets.len(), 1);
}

#[test]
fn ws_builds_its_sidecar_and_init_images() {
    let targets = DeployTarget::resolve(&["ws".to_string()]).expect("ws is selectable");
    let images = DeployTarget::images_for(&targets);
    assert!(images.contains(&"runinator-ws"));
    assert!(images.contains(&"runinator-adapter-host"));
    assert!(images.contains(&"runinator-bootstrap"));
}

#[test]
fn shared_sidecar_images_are_built_once() {
    let keys = vec!["ws".to_string(), "engine-worker".to_string()];
    let targets = DeployTarget::resolve(&keys).expect("both services are selectable");
    let images = DeployTarget::images_for(&targets);
    let adapter_hosts = images
        .iter()
        .filter(|image| **image == "runinator-adapter-host")
        .count();
    assert_eq!(adapter_hosts, 1);
}

#[test]
fn a_service_owns_its_extra_named_resources() {
    let targets = DeployTarget::resolve(&["archiver".to_string()]).expect("archiver is selectable");
    let resources = DeployTarget::resources_for(&targets);
    assert_eq!(
        resources,
        vec!["runinator-archiver", "runinator-archive-data"]
    );
}
