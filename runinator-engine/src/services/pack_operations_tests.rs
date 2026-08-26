use std::{path::PathBuf, sync::Arc};

use runinator_blob_core::{BlobStore, FUNCTION_ARTIFACT_BUCKET, FsBlobStore, sha256_hex};
use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    functions::{
        FunctionBinding, FunctionRuntimeSpec, NewFunctionExport, NewFunctionPackage,
        NewFunctionVersion, PROVISIONAL_FUNCTION_VERSION, digest_from_hex,
    },
    json,
    semver::SemVer,
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{
    DatabaseImpl,
    roles::{DefinitionStore, FunctionStore},
};
use uuid::Uuid;

use super::*;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path =
        std::env::temp_dir().join(format!("runinator-pack-operations-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn workflow_with_binding(binding: FunctionBinding) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: "same-pack function".into(),
        key: None,
        namespace: None,
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
                    "kind": "action",
                    "action": {
                        "provider": "functions",
                        "function": "invoke",
                        "function_binding": binding
                    },
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
async fn resolves_a_same_pack_function_binding_to_the_published_uuid_tuple() {
    let (db, db_path) = test_db().await;
    let blob_root = std::env::temp_dir().join(format!("runinator-pack-blobs-{}", Uuid::now_v7()));
    let blobs = FsBlobStore::open(&blob_root).await.unwrap();
    blobs.create_bucket(FUNCTION_ARTIFACT_BUCKET).await.unwrap();
    let broker = Arc::new(InMemoryBroker::new());
    let service = PackOperations::new(db.clone(), Arc::new(blobs), UiEventPublisher::new(broker));

    let bytes = b"same-pack function archive".to_vec();
    let digest = digest_from_hex(&sha256_hex(&bytes));
    service
        .put_function_artifact_if_absent(&digest, bytes)
        .await
        .unwrap();
    let published = service
        .publish_function(&NewFunctionVersion {
            package: NewFunctionPackage {
                name: "pdf".into(),
                namespace: Some("acme.shared".into()),
                description: None,
                org_id: None,
            },
            artifact_digest: digest.clone(),
            manifest: Value::Null,
            runtime: FunctionRuntimeSpec::new("python3.13"),
            exports: vec![NewFunctionExport {
                name: "render".into(),
                handler: "render".into(),
                description: None,
                input: Vec::new(),
                output: Vec::new(),
                limits: Default::default(),
            }],
            alias: Some("latest".into()),
        })
        .await
        .unwrap();
    let actual = db
        .fetch_function_catalog()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.version_id == published.id && entry.export_name == "render")
        .expect("published export in catalog")
        .binding();
    let provisional = FunctionBinding {
        package_id: Uuid::now_v7(),
        package_name: "pdf".into(),
        namespace: Some("acme.shared".into()),
        version_id: Uuid::now_v7(),
        version: PROVISIONAL_FUNCTION_VERSION,
        export_id: Uuid::now_v7(),
        export_name: "render".into(),
        artifact_digest: digest,
    };
    let mut bundle = WorkflowBundle {
        workflows: vec![workflow_with_binding(provisional)],
        triggers: Vec::new(),
    };

    service
        .resolve_provisional_function_bindings(&mut bundle, &[published])
        .await
        .unwrap();

    let binding = bundle.workflows[0].definition.nodes[1]
        .action
        .as_ref()
        .and_then(|action| action.function_binding.as_ref())
        .expect("resolved function binding");
    assert_eq!(binding, &actual);
    assert!(!binding.is_provisional());

    let package_id = actual.package_id;
    let moved = crate::repository::functions::move_package(
        db.as_ref(),
        package_id,
        Some("acme.documents".into()),
        "pdf-tools".into(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(moved.id, package_id);
    assert_eq!(moved.qualified_name(), "acme.documents.pdf-tools");
    assert!(
        db.fetch_catalog_item(crate::repository::provider_catalog_uri(
            "functions.acme.shared.pdf"
        ))
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        db.fetch_catalog_item(crate::repository::provider_catalog_uri(
            "functions.acme.documents.pdf-tools"
        ))
        .await
        .unwrap()
        .is_some()
    );
    // The already-compiled call is an exact version/export UUID tuple and does not change.
    assert_eq!(binding, &actual);

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(blob_root);
}
