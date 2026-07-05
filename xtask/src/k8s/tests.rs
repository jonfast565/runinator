use std::collections::HashMap;
use std::path::PathBuf;

use super::images::{image_tag, versioned_image_tag};
use super::kustomize::{
    add_component, overlay_has_image, set_overlay_images, split_image_reference,
};
use super::yaml_docs::{
    filter_out_statefulsets, parse_documents, rollout_target, select_by_names, workload_kind,
};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xtask-k8s-test-{tag}-{}-{}",
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
fn split_image_reference_splits_on_the_last_colon_after_the_last_slash() {
    let plain = split_image_reference("runinator-ws:dev").unwrap();
    assert_eq!(plain.name, "runinator-ws");
    assert_eq!(plain.tag, "dev");

    let with_port = split_image_reference("registry.example.com:5000/runinator-ws:1.2.3").unwrap();
    assert_eq!(with_port.name, "registry.example.com:5000/runinator-ws");
    assert_eq!(with_port.tag, "1.2.3");
}

#[test]
fn split_image_reference_requires_a_tag() {
    assert!(split_image_reference("no-tag").is_err());
    assert!(split_image_reference("registry.example.com:5000/no-tag").is_err());
}

#[test]
fn versioned_image_tag_generates_a_fresh_tag_for_local_or_empty() {
    assert!(versioned_image_tag("local").starts_with("kube-"));
    assert!(versioned_image_tag("").starts_with("kube-"));
    assert_eq!(versioned_image_tag("1.0.0"), "1.0.0");
}

#[test]
fn image_tag_prefixes_a_repository_only_when_present() {
    assert_eq!(image_tag("runinator-ws", None, "dev"), "runinator-ws:dev");
    assert_eq!(
        image_tag(
            "runinator-ws",
            Some("registry.example.com/runinator"),
            "dev"
        ),
        "registry.example.com/runinator/runinator-ws:dev"
    );
}

const FIXTURE_KUSTOMIZATION: &str = "\
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - ../../base
# comment above the images list should survive.
images:
  - name: runinator-ws
    newName: runinator-ws
    newTag: dev
  - name: runinator-worker
    newName: runinator-worker
    newTag: dev
";

#[test]
fn set_overlay_images_rewrites_matching_entries_and_preserves_comments() {
    let dir = temp_dir("set-images");
    std::fs::write(dir.join("kustomization.yaml"), FIXTURE_KUSTOMIZATION).unwrap();

    let mut image_map = HashMap::new();
    image_map.insert(
        "runinator-ws".to_string(),
        "registry.example.com/runinator-ws:kube-20260704000000".to_string(),
    );
    set_overlay_images(&dir, &image_map).unwrap();

    let updated = std::fs::read_to_string(dir.join("kustomization.yaml")).unwrap();
    assert!(updated.contains("# comment above the images list should survive."));
    assert!(updated.contains("newName: registry.example.com/runinator-ws"));
    assert!(updated.contains("newTag: kube-20260704000000"));
    // the untouched entry keeps its original override.
    assert!(updated.contains("newName: runinator-worker"));
    assert!(updated.contains("newTag: dev"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn set_overlay_images_errors_on_an_image_the_kustomization_does_not_declare() {
    let dir = temp_dir("set-images-missing");
    std::fs::write(dir.join("kustomization.yaml"), FIXTURE_KUSTOMIZATION).unwrap();

    let mut image_map = HashMap::new();
    image_map.insert("runinator-does-not-exist".to_string(), "x:y".to_string());
    assert!(set_overlay_images(&dir, &image_map).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn overlay_has_image_matches_declared_names_only() {
    let dir = temp_dir("has-image");
    std::fs::write(dir.join("kustomization.yaml"), FIXTURE_KUSTOMIZATION).unwrap();

    assert!(overlay_has_image(&dir, "runinator-ws"));
    assert!(!overlay_has_image(&dir, "runinator-command-center-flutter"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_component_creates_a_components_block_when_absent() {
    let workspace_root = temp_dir("add-component-workspace");
    let component_dir = workspace_root.join("target/k8s-render/k8s/components/direct-ingress");
    std::fs::create_dir_all(&component_dir).unwrap();
    std::fs::write(component_dir.join("kustomization.yaml"), "resources: []\n").unwrap();

    let overlay_dir = workspace_root.join("target/k8s-render/k8s/overlays/local");
    std::fs::create_dir_all(&overlay_dir).unwrap();
    std::fs::write(
        overlay_dir.join("kustomization.yaml"),
        FIXTURE_KUSTOMIZATION,
    )
    .unwrap();

    add_component(&workspace_root, &overlay_dir, "components/direct-ingress").unwrap();

    let updated = std::fs::read_to_string(overlay_dir.join("kustomization.yaml")).unwrap();
    assert!(updated.contains("components:"));
    assert!(updated.contains("- ../../components/direct-ingress"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

const FIXTURE_RENDERED_MANIFEST: &str = "\
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: runinator-postgres
  namespace: runinator
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: runinator-rabbitmq
  namespace: runinator
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: runinator-ws
  namespace: runinator
---
apiVersion: v1
kind: Service
metadata:
  name: runinator-ws
  namespace: runinator
";

#[test]
fn parse_documents_splits_on_document_separators() {
    let docs = parse_documents(FIXTURE_RENDERED_MANIFEST).unwrap();
    assert_eq!(docs.len(), 4);
}

#[test]
fn filter_out_statefulsets_drops_only_named_statefulsets() {
    let docs = parse_documents(FIXTURE_RENDERED_MANIFEST).unwrap();
    let filtered = filter_out_statefulsets(&docs, &["runinator-postgres"]);
    assert_eq!(filtered.len(), 3);
    assert!(!filtered.iter().any(
        |doc| doc.get("metadata").unwrap().get("name").unwrap().as_str()
            == Some("runinator-postgres")
    ));
    // the rabbitmq StatefulSet was not named, so it survives.
    assert!(filtered.iter().any(
        |doc| doc.get("metadata").unwrap().get("name").unwrap().as_str()
            == Some("runinator-rabbitmq")
    ));
}

#[test]
fn select_by_names_keeps_only_matching_documents_regardless_of_kind() {
    let docs = parse_documents(FIXTURE_RENDERED_MANIFEST).unwrap();
    let selected = select_by_names(&docs, &["runinator-ws"]);
    assert_eq!(
        selected.len(),
        2,
        "both the Deployment and Service named runinator-ws should match"
    );
}

#[test]
fn workload_kind_and_rollout_target_prefer_the_rendered_kind_over_the_fallback() {
    let docs = parse_documents(FIXTURE_RENDERED_MANIFEST).unwrap();
    assert_eq!(
        workload_kind(&docs, "runinator-ws").as_deref(),
        Some("Deployment")
    );
    assert_eq!(
        rollout_target(&docs, "runinator-ws", "StatefulSet"),
        "deployment/runinator-ws"
    );
    // a name with no rendered Deployment/StatefulSet doc falls back.
    assert_eq!(
        rollout_target(&docs, "runinator-worker", "StatefulSet"),
        "statefulset/runinator-worker"
    );
}
