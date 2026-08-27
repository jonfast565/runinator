use std::{path::PathBuf, sync::Arc};

use runinator_blob_core::{BlobStore, FUNCTION_ARTIFACT_BUCKET, FsBlobStore, sha256_hex};
use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    functions::{
        FunctionBinding, FunctionRuntimeSpec, NewFunctionExport, NewFunctionPackage,
        NewFunctionVersion, PROVISIONAL_FUNCTION_VERSION, digest_from_hex,
    },
    json,
    pipelines::{PipelineBundle, PipelineSpec},
    semver::SemVer,
    settings::SettingKind,
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{
    DatabaseImpl, RuntimeStore,
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

#[tokio::test]
async fn late_pipeline_failure_rolls_back_settings_and_workflows() {
    let (db, db_path) = test_db().await;
    let blob_root = std::env::temp_dir().join(format!("runinator-pack-blobs-{}", Uuid::now_v7()));
    let blobs = Arc::new(FsBlobStore::open(&blob_root).await.unwrap());
    blobs.create_bucket(FUNCTION_ARTIFACT_BUCKET).await.unwrap();
    let service = PackOperations::new(
        db.clone(),
        blobs,
        UiEventPublisher::new(Arc::new(InMemoryBroker::new())),
    );
    let function_bytes = b"rollback package".to_vec();
    let function_digest = digest_from_hex(&sha256_hex(&function_bytes));
    let artifact = service
        .stage_function_artifact(&function_digest, function_bytes)
        .await
        .unwrap();
    let functions = vec![NewFunctionVersion {
        package: NewFunctionPackage {
            name: "pdf".into(),
            namespace: Some("acme.shared".into()),
            description: None,
            org_id: None,
        },
        artifact_digest: function_digest.clone(),
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
    }];
    let workflows = WorkflowBundle {
        workflows: vec![WorkflowDefinition {
            id: None,
            name: "Reconcile invoices".into(),
            key: Some("reconcile".into()),
            namespace: Some("acme.billing".into()),
            org_id: None,
            version: SemVer::new(1, 0, 0),
            enabled: true,
            input_type: RuninatorType::Any,
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
        triggers: Vec::new(),
    };
    let settings = SecretBundle {
        secrets: vec![SecretBundleEntry {
            scope: "acme.shared".into(),
            name: "api_token".into(),
            value: Value::String("secret".into()),
            schema: None,
            kind: SettingKind::Secret,
            updated_at: None,
            expires_at: None,
        }],
    };
    let pipelines = PipelineBundle {
        pipelines: vec![PipelineSpec {
            name: "Broken release".into(),
            key: Some("broken_release".into()),
            namespace: Some("acme.delivery".into()),
            description: None,
            defaults: Default::default(),
            members: vec!["acme.billing.missing".into()],
            links: Vec::new(),
            joins: Vec::new(),
            concurrency: Default::default(),
            metadata: runinator_models::json!({}),
            triggers: Vec::new(),
        }],
    };

    let result = service
        .import_compiled_pack(
            workflows,
            Some(&settings),
            Some(&pipelines),
            &functions,
            &[artifact],
            None,
            true,
        )
        .await;

    assert!(result.is_err(), "the unresolved pipeline member must fail");
    assert!(db.fetch_workflows().await.unwrap().is_empty());
    assert!(
        db.fetch_function_package(None, Some("acme.shared"), "pdf")
            .await
            .unwrap()
            .is_none(),
        "a function published before the pipeline failure must roll back"
    );
    assert!(
        db.fetch_function_artifact(&function_digest)
            .await
            .unwrap()
            .is_none(),
        "the staged blob's database descriptor must roll back with the pack"
    );
    assert!(
        db.fetch_setting(
            SettingKind::Secret,
            "acme.shared".into(),
            "api_token".into(),
        )
        .await
        .unwrap()
        .is_none(),
        "a setting written before the pipeline failure must roll back"
    );

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(blob_root);
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
