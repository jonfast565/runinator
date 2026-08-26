use std::{path::PathBuf, sync::Arc};

use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    json,
    revisions::{RevisionAuthor, RevisionSource},
    semver::SemVer,
    settings::{SettingBinding, SettingKind},
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{DatabaseImpl, RuntimeStore, roles::SettingStore};

use super::*;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-authoring-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.into(),
        key: None,
        namespace: None,
        org_id: None,
        version: SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn subflow_workflow(name: &str, target: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.into(),
        key: None,
        namespace: Some("acme.billing".into()),
        org_id: None,
        version: SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "call" } } },
                {
                    "id": "call",
                    "kind": "subflow",
                    "subflow": { "workflow_name": target },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn save_persists_an_authored_workflow() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker.clone()));

    let saved = service
        .save(
            &workflow("authoring service"),
            &RevisionAuthor::system(RevisionSource::Api),
        )
        .await
        .unwrap();
    let workflow_id = saved.id.expect("saved workflow receives an id");
    assert!(db.fetch_workflow(workflow_id).await.unwrap().is_some());

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn saving_a_subflow_resolves_its_path_to_a_durable_reference() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker));
    let author = RevisionAuthor::system(RevisionSource::Api);

    let mut target = workflow("reconcile");
    target.namespace = Some("acme.billing".into());
    let target = service.save(&target, &author).await.unwrap();
    let target_id = target.id.unwrap();

    let caller = service
        .save(
            &subflow_workflow("caller", "acme.billing.reconcile"),
            &author,
        )
        .await
        .unwrap();
    let call = caller
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "call")
        .expect("caller has a subflow node");
    let reference = call
        .subflow
        .target
        .as_ref()
        .expect("artifact ref was written");
    assert_eq!(reference.id, target_id);
    assert_eq!(
        reference.authored_path.as_ref().unwrap().qualified(),
        "acme.billing.reconcile"
    );
    assert_eq!(call.subflow_id, Some(target_id));

    let error = service.delete(target_id).await.unwrap_err();
    assert!(error.to_string().contains("inbound durable references"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn saving_a_revision_selector_records_the_target_digest_pin() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker));
    let author = RevisionAuthor::system(RevisionSource::Api);

    let mut target = workflow("reconcile");
    target.namespace = Some("acme.billing".into());
    let target = service.save(&target, &author).await.unwrap();
    let revision = service
        .revisions(target.id.unwrap(), 1)
        .await
        .unwrap()
        .pop()
        .expect("target revision");
    assert!(revision.digest.starts_with("sha256:"));

    let mut caller = subflow_workflow("pinned caller", "acme.billing.reconcile");
    caller
        .definition
        .nodes
        .iter_mut()
        .find(|node| node.id == "call")
        .unwrap()
        .subflow
        .revision = Some(revision.revision);
    let caller = service.save(&caller, &author).await.unwrap();
    let pin = caller
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "call")
        .and_then(|node| node.subflow.target.as_ref())
        .and_then(|reference| reference.revision_pin.as_ref())
        .expect("revision selector became an exact pin");
    assert_eq!(pin.revision, revision.revision);
    assert_eq!(pin.digest, revision.digest);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn saving_a_chained_trigger_resolves_and_blocks_deleting_its_target() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker));
    let author = RevisionAuthor::system(RevisionSource::Api);

    let mut target = workflow("deploy");
    target.key = Some("deploy".into());
    target.namespace = Some("acme.release".into());
    let target = service.save(&target, &author).await.unwrap();
    let target_id = target.id.unwrap();

    let mut caller = workflow("build");
    caller.definition.metadata = json!({
        "triggers": [{
            "kind": "chained",
            "target_workflow": "acme.release.deploy",
            "on": "success"
        }]
    });
    let caller = service.save(&caller, &author).await.unwrap();
    let trigger = caller.definition.metadata.pointer("/triggers/0").unwrap();
    let target_id_text = target_id.to_string();
    assert_eq!(
        trigger.get("target_workflow_id").and_then(Value::as_str),
        Some(target_id_text.as_str())
    );
    assert_eq!(
        trigger.pointer("/target/id").and_then(Value::as_str),
        Some(target_id_text.as_str())
    );

    let error = service.delete(target_id).await.unwrap_err();
    assert!(error.to_string().contains("inbound durable references"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn saving_setting_references_records_their_durable_uuids() {
    let (db, path) = test_db().await;
    db.upsert_setting(
        SettingKind::Config,
        "acme.shared".into(),
        "message".into(),
        b"{}".to_vec(),
        1,
    )
    .await
    .unwrap();
    db.upsert_setting(
        SettingKind::Secret,
        "acme.shared".into(),
        "token".into(),
        b"encrypted".to_vec(),
        1,
    )
    .await
    .unwrap();
    let config_id = db
        .fetch_setting(SettingKind::Config, "acme.shared".into(), "message".into())
        .await
        .unwrap()
        .unwrap()
        .id;
    let secret_id = db
        .fetch_setting(SettingKind::Secret, "acme.shared".into(), "token".into())
        .await
        .unwrap()
        .unwrap()
        .id;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker));
    let mut definition = workflow("setting consumer");
    definition.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "emit" } } },
            {
                "id": "emit",
                "kind": "output",
                "parameters": {
                    "data": {
                        "text": { "$ref": { "config": ["acme.shared", "message"] } },
                        "token": "secret://acme.shared/token"
                    }
                },
                "transitions": { "next": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();
    let saved = service
        .save(&definition, &RevisionAuthor::system(RevisionSource::Api))
        .await
        .unwrap();
    let bindings = saved
        .definition
        .metadata
        .pointer("/artifact_refs/settings")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|value| serde_json::from_value::<SettingBinding>(value.clone().into()).unwrap())
        .collect::<Vec<_>>();
    assert!(bindings.iter().any(|binding| {
        binding.kind == SettingKind::Config && binding.reference.id == config_id
    }));
    assert!(bindings.iter().any(|binding| {
        binding.kind == SettingKind::Secret && binding.reference.id == secret_id
    }));
    assert_eq!(
        saved
            .definition
            .as_value()
            .pointer("/nodes/1/parameters/data/token")
            .and_then(Value::as_str),
        Some(format!("secret+uuid://{secret_id}/acme.shared/token").as_str())
    );

    db.move_setting(
        config_id,
        SettingKind::Config,
        "acme.moved".into(),
        "message".into(),
    )
    .await
    .unwrap();
    let config = runinator_runtime::config::config_tree_for_workflow(db.as_ref(), &saved).await;
    assert!(config.pointer("/acme.shared/message").is_some());

    let _ = std::fs::remove_file(path);
}
