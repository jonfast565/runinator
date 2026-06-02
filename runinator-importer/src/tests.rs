use std::sync::Mutex;

use async_trait::async_trait;
use runinator_models::json;

use super::{
    ImporterServiceLocator, WorkflowBundleImporter, build_provider_bundle, build_service_locator,
    config::Config, load_import_file, load_secret_bundle, sync_workflows_if_changed,
    workflow_bundle_path,
};
use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
use runinator_models::workflows::WorkflowBundle;
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowGraph, WorkflowTrigger, WorkflowTriggerKind,
};

#[tokio::test]
async fn sync_imports_clean_workflow_bundle_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-importer-bundle-{}.json",
        uuid::Uuid::new_v4()
    ));
    let expected = clean_bundle();
    tokio::fs::write(&path, serde_json::to_vec(&expected).unwrap())
        .await
        .unwrap();

    let config = Config {
        api_base_url: None,
        workflows_file: Some(path.to_string_lossy().into_owned()),
        secrets_file: None,
        poll_interval_seconds: 10,
        gossip_bind: "127.0.0.1".into(),
        gossip_port: 5000,
        gossip_targets: Vec::new(),
        once: true,
    };
    let api = RecordingImporter::default();
    let mut last_modified = None;

    sync_workflows_if_changed(&config, &api, &mut last_modified)
        .await
        .unwrap();

    let imported = api.imported.lock().unwrap().clone().unwrap();
    assert_eq!(
        serde_json::to_value(imported).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(last_modified.is_some());
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn load_secret_bundle_reads_credentials() {
    let path = std::env::temp_dir().join(format!(
        "runinator-importer-secrets-{}.json",
        uuid::Uuid::new_v4()
    ));
    let expected = SecretBundle {
        secrets: vec![SecretBundleEntry {
            scope: "github".into(),
            name: "main".into(),
            value: runinator_models::value::Value::String("token".into()),
            schema: None,
            kind: Default::default(),
            updated_at: None,
        }],
    };
    tokio::fs::write(&path, serde_json::to_vec(&expected).unwrap())
        .await
        .unwrap();

    let bundle = load_secret_bundle(&path).await.unwrap();

    assert_eq!(
        serde_json::to_value(bundle).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn sync_reports_missing_workflow_bundle_path() {
    let path = std::env::temp_dir().join(format!(
        "runinator-importer-missing-{}.json",
        uuid::Uuid::new_v4()
    ));
    let config = Config {
        api_base_url: None,
        workflows_file: Some(path.to_string_lossy().into_owned()),
        secrets_file: None,
        poll_interval_seconds: 10,
        gossip_bind: "127.0.0.1".into(),
        gossip_port: 5000,
        gossip_targets: Vec::new(),
        once: true,
    };
    let api = RecordingImporter::default();
    let mut last_modified = None;

    let err = sync_workflows_if_changed(&config, &api, &mut last_modified)
        .await
        .expect_err("missing workflow bundle should fail");

    assert!(err.to_string().contains(path.to_string_lossy().as_ref()));
}

#[tokio::test]
async fn configured_api_base_url_uses_static_locator_without_binding_gossip() {
    let config = Config {
        api_base_url: Some("http://runinator-ws.runinator.svc.cluster.local:8080/".into()),
        workflows_file: None,
        secrets_file: None,
        poll_interval_seconds: 10,
        gossip_bind: "127.0.0.1".into(),
        gossip_port: 9,
        gossip_targets: Vec::new(),
        once: true,
    };

    let locator = build_service_locator(&config).await.unwrap();

    assert!(matches!(locator, ImporterServiceLocator::Static(_)));
}

#[test]
fn workflow_bundle_default_path_uses_wdlp_manifest() {
    let config = Config {
        api_base_url: None,
        workflows_file: None,
        secrets_file: None,
        poll_interval_seconds: 10,
        gossip_bind: "127.0.0.1".into(),
        gossip_port: 5000,
        gossip_targets: Vec::new(),
        once: true,
    };

    let path = workflow_bundle_path(&config);

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("sdlc.wdlp")
    );
    assert!(path.to_string_lossy().contains("workflows"));
}

#[tokio::test]
async fn load_import_file_compiles_wdl_directory() {
    let dir = std::env::temp_dir().join(format!(
        "runinator-importer-wdldir-{}",
        uuid::Uuid::new_v4()
    ));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(
        dir.join("alpha.wdl"),
        "workflow \"Alpha\" v1 {\n  let a = console.run(command: \"a\")\n}\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        dir.join("beta.wdl"),
        "workflow \"Beta\" v1 {\n  let b = console.run(command: \"b\")\n}\n",
    )
    .await
    .unwrap();
    // a non-wdl file is ignored.
    tokio::fs::write(dir.join("README.md"), "ignore me")
        .await
        .unwrap();

    let bundle = load_import_file(&dir).await.unwrap();

    assert_eq!(bundle.workflows.len(), 2);
    assert!(bundle.triggers.is_empty());
    let names: Vec<_> = bundle.workflows.iter().map(|w| w.name.clone()).collect();
    assert!(names.contains(&"Alpha".to_string()));
    assert!(names.contains(&"Beta".to_string()));
    assert!(bundle.workflows.iter().all(|w| w.enabled));

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn load_import_file_resolves_wdlp_manifest_with_triggers() {
    let dir =
        std::env::temp_dir().join(format!("runinator-importer-wdlp-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(
        dir.join("alpha.wdl"),
        "workflow \"Alpha\" v1 {\n  let a = console.run(command: \"a\")\n}\n",
    )
    .await
    .unwrap();
    let manifest = json!({
        "item_type": "wdl_pack",
        "name": "Sample",
        "version": 4,
        "workflows": ["alpha.wdl"],
        "triggers": [{
            "id": null,
            "workflow_id": 0,
            "kind": "manual",
            "enabled": true,
            "configuration": {},
            "metadata": {}
        }]
    });
    let manifest_path = dir.join("pack.wdlp");
    tokio::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap())
        .await
        .unwrap();

    let bundle = load_import_file(&manifest_path).await.unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].name, "Alpha");
    // the manifest version flows through as the default when the source omits vN.
    assert_eq!(bundle.workflows[0].version, 1); // source declared v1, which wins
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].kind, WorkflowTriggerKind::Manual);

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn sdlc_wdlp_pack_compiles_both_workflows() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/sdlc/sdlc.wdlp");
    let bundle = load_import_file(&manifest).await.expect("wdlp pack loads");
    let names: Vec<_> = bundle.workflows.iter().map(|w| w.name.clone()).collect();
    assert!(names.contains(&"Core Team SDLC Pipeline".to_string()));
    assert!(names.contains(&"Ticket Work".to_string()));
    assert!(bundle.workflows.iter().all(|w| w.enabled));
}

#[test]
fn provider_bundle_includes_every_provider() {
    let bundle = build_provider_bundle();
    let names: Vec<_> = bundle.providers.iter().map(|p| p.name.as_str()).collect();
    for expected in [
        "Console",
        "AWS",
        "SQL",
        "jira",
        "github",
        "slack",
        "git",
        "email",
        "ai-command",
        "approval",
    ] {
        assert!(
            names.contains(&expected),
            "missing provider '{expected}' in {names:?}"
        );
    }
}

#[derive(Default)]
struct RecordingImporter {
    imported: Mutex<Option<WorkflowBundle>>,
}

#[async_trait]
impl WorkflowBundleImporter for RecordingImporter {
    async fn import_workflow_bundle(
        &self,
        bundle: &WorkflowBundle,
    ) -> runinator_api::Result<WorkflowBundle> {
        *self.imported.lock().unwrap() = Some(bundle.clone());
        Ok(bundle.clone())
    }
}

fn clean_bundle() -> WorkflowBundle {
    WorkflowBundle {
        workflows: vec![WorkflowDefinition {
            id: Some(77),
            name: "clean".into(),
            version: 1,
            enabled: true,
            input_type: runinator_models::types::RuninatorType::from_json_schema(
                &json!({ "type": "object" }),
            ),
            definition: WorkflowGraph::from_value(json!({
                "start": "start",
                "nodes": [
                    { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                    { "id": "done", "kind": "end" }
                ]
            }))
            .unwrap(),
            created_at: None,
            updated_at: None,
        }],
        triggers: vec![WorkflowTrigger {
            id: Some(88),
            workflow_id: 77,
            kind: WorkflowTriggerKind::Manual,
            enabled: true,
            configuration: json!({}),
            next_execution: None,
            blackout_start: None,
            blackout_end: None,
            metadata: json!({ "source": "test" }),
            created_at: None,
            updated_at: None,
        }],
    }
}
