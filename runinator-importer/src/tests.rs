use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use super::{
    WorkflowBundleImporter, build_provider_bundle, config::Config, load_import_file,
    load_secret_bundle, sync_workflows_if_changed, unwrap_workflow_pack,
};
use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
use runinator_models::workflows::WorkflowBundle;
use runinator_models::workflows::{WorkflowDefinition, WorkflowTrigger, WorkflowTriggerKind};

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
            secret: "token".into(),
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
async fn load_import_file_unwraps_workflow_pack_envelope() {
    let path = std::env::temp_dir().join(format!(
        "runinator-importer-pack-{}.json",
        uuid::Uuid::new_v4()
    ));
    let envelope = json!({
        "uri": "runinator://packs/sample",
        "item_type": "workflow_pack",
        "name": "Sample Pack",
        "version": "4",
        "document": {
            "workflows": {
                "alpha": {
                    "metadata": { "description": "first" },
                    "input_schema": { "type": "object" },
                    "start": "n1",
                    "nodes": [{ "id": "n1", "kind": "end" }]
                },
                "beta": {
                    "input_schema": { "type": "object" },
                    "start": "x",
                    "nodes": [{ "id": "x", "kind": "end" }]
                }
            }
        }
    });
    tokio::fs::write(&path, serde_json::to_vec(&envelope).unwrap())
        .await
        .unwrap();

    let bundle = load_import_file(&path).await.unwrap();

    assert_eq!(bundle.workflows.len(), 2);
    assert!(bundle.triggers.is_empty());
    let alpha = bundle
        .workflows
        .iter()
        .find(|w| w.name == "alpha")
        .expect("alpha workflow present");
    assert_eq!(alpha.version, 4);
    assert!(alpha.enabled);
    assert_eq!(alpha.input_schema, json!({ "type": "object" }));
    assert_eq!(alpha.definition["start"], json!("n1"));
    assert_eq!(alpha.definition["metadata"]["description"], json!("first"));

    let _ = tokio::fs::remove_file(path).await;
}

#[test]
fn unwrap_workflow_pack_rejects_missing_document() {
    let envelope = json!({
        "uri": "runinator://packs/broken",
        "item_type": "workflow_pack",
        "name": "Broken Pack",
        "version": "1"
    });
    let err = unwrap_workflow_pack(envelope).expect_err("missing document should error");
    assert!(err.to_string().contains("document"));
}

#[test]
fn sdlc_pack_unwraps_to_workflow_bundle() {
    let raw = include_str!("../../packs/sdlc/workflow-pack.json");
    let envelope: serde_json::Value = serde_json::from_str(raw).expect("pack file parses");
    let bundle = unwrap_workflow_pack(envelope).expect("pack unwraps");
    assert!(
        !bundle.workflows.is_empty(),
        "pack has at least one workflow"
    );
    let names = bundle
        .workflows
        .iter()
        .map(|workflow| workflow.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"Core Team SDLC Pipeline"));
    assert!(names.contains(&"Ticket Work"));
    let provider_bundle = build_provider_bundle();
    for workflow in &bundle.workflows {
        assert!(workflow.enabled);
        assert!(workflow.version >= 1);
        assert!(workflow.definition.is_object());
        runinator_workflows::validate_workflow_with_providers(workflow, &provider_bundle.providers)
            .expect("sdlc workflow validates");
    }
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
            input_schema: json!({ "type": "object" }),
            definition: json!({
                "start": "start",
                "nodes": [
                    { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                    { "id": "done", "kind": "end" }
                ]
            }),
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
