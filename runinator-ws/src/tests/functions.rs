//! covers the publish path end to end through the handlers: archive a package directory, upload its
//! bytes by digest, publish a version, and move an alias.
//!
//! the gate this phase exists for is here — the same tree publishes to the same digest, and
//! republishing identical bytes stores nothing new.

use super::*;

use std::sync::Arc;

use runinator_blob::{BlobStore, FUNCTION_ARTIFACT_BUCKET, FsBlobStore};
use runinator_engine::services::{FunctionInvocations, FunctionPackages};
use runinator_models::functions::{FunctionVersionRef, NewFunctionVersion};
use runinator_pack::functions::{FunctionSource, MANIFEST_FILE, archive_directory};
use runinator_ws_middleware::authz::AuthContextExt;

use crate::models::ApiResponse;

const MANIFEST: &str = r#"{
  "name": "image-tools",
  "namespace": "runinator.examples",
  "description": "image utilities",
  "runtime": { "runtime": "python3.13" },
  "exports": [
    {
      "name": "resize",
      "handler": "src.images.resize",
      "input": [{ "name": "source", "type": "string", "required": true }],
      "output": [{ "name": "uri", "type": "string" }]
    },
    { "name": "inspect", "handler": "src.images.inspect" }
  ]
}"#;

const PACKAGE_NAMESPACE: &str = "runinator.examples";
const PACKAGE_PATH: &str = "runinator.examples.image-tools";

fn admin_ctx() -> AuthContext {
    AuthContext {
        principal_id: Some(Uuid::new_v4()),
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    }
}

fn function_packages(
    db: &Arc<SqliteDb>,
    blobs: &Arc<dyn BlobStore>,
) -> Extension<Arc<FunctionPackages<SqliteDb>>> {
    Extension(Arc::new(FunctionPackages::new(db.clone(), blobs.clone())))
}

fn function_invocations(
    db: &Arc<SqliteDb>,
    events: &crate::events::EventSender,
) -> Extension<Arc<FunctionInvocations<SqliteDb>>> {
    Extension(Arc::new(FunctionInvocations::new(
        db.clone(),
        Arc::new(InMemoryBroker::new()),
        events.publisher(),
        events.embedded_engine_signals(),
    )))
}

// a package directory on disk, plus the blob store the handlers write its bytes into.
async fn fixture(name: &str) -> (std::path::PathBuf, Arc<dyn BlobStore>, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("runi-ws-fn-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join(MANIFEST_FILE), MANIFEST).unwrap();
    std::fs::write(
        root.join("src/images.py"),
        "def resize(source):\n    return source\n",
    )
    .unwrap();

    let blob_root = std::env::temp_dir().join(format!("runi-ws-blob-{}", Uuid::new_v4()));
    let store = FsBlobStore::open(&blob_root).await.unwrap();
    store.create_bucket(FUNCTION_ARTIFACT_BUCKET).await.unwrap();
    (root, Arc::new(store) as Arc<dyn BlobStore>, blob_root)
}

#[tokio::test]
async fn publishing_uploads_by_digest_and_republishing_stores_nothing_new() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("publish").await;

    let source = FunctionSource::load(&root).unwrap();
    let digest = source.archive.digest.clone();

    // the client's probe: nothing stored yet, so the upload is required.
    let (status, _) = crate::handlers::functions::get_function_artifact::<SqliteDb>(
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(digest.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stored_uri = match body.0 {
        ApiResponse::FunctionArtifact(artifact) => {
            assert_eq!(artifact.digest, digest);
            assert_eq!(artifact.size_bytes, source.archive.size_bytes());
            artifact.uri
        }
        _ => panic!("unexpected response"),
    };

    // uploading the identical bytes again returns the same record rather than writing a second copy.
    let (status, body) = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::FunctionArtifact(artifact) => assert_eq!(artifact.uri, stored_uri),
        _ => panic!("unexpected response"),
    }

    // publish, then publish the same tree again: two versions, one artifact.
    let request = source.publish_request();
    for expected in [1i64, 2] {
        let (status, body) = crate::handlers::functions::publish_function::<SqliteDb>(
            Extension(db.clone()),
            function_packages(&db, &blobs),
            Extension(admin_ctx()),
            ValidatedJson(request.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        match body.0 {
            ApiResponse::FunctionVersion(version) => {
                assert_eq!(version.version, expected);
                assert_eq!(version.artifact_digest, digest);
            }
            _ => panic!("unexpected response"),
        }
    }

    let (status, body) = crate::handlers::functions::get_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(PACKAGE_PATH.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::FunctionPackage(detail) => {
            assert_eq!(detail.versions.len(), 2);
            // both exports survived the round trip, and the default alias followed the newest publish.
            assert_eq!(detail.exports.len(), 2);
            let latest = detail
                .aliases
                .iter()
                .find(|alias| alias.name == "latest")
                .expect("latest alias");
            assert_eq!(latest.version, 2);
        }
        _ => panic!("unexpected response"),
    }

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn a_publish_without_its_bytes_is_refused() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("no-bytes").await;

    // the artifact was never uploaded, so publishing it would create a version nothing can run.
    let request = FunctionSource::load(&root).unwrap().publish_request();
    let (status, _body) = crate::handlers::functions::publish_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        ValidatedJson(request),
    )
    .await;
    assert_ne!(status, StatusCode::OK);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn bytes_that_disagree_with_their_digest_are_refused() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("mismatch").await;
    let digest = archive_directory(&root, &[]).unwrap().digest;

    // the digest is what pins a workflow to exact code, so a caller cannot name one for other bytes.
    let (status, _) = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(digest),
        axum::body::Bytes::from_static(b"not the archive"),
    )
    .await;
    assert_ne!(status, StatusCode::OK);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn moving_an_alias_leaves_earlier_versions_where_they_are() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("alias").await;
    let source = FunctionSource::load(&root).unwrap();

    let _ = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(source.archive.digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    let mut request: NewFunctionVersion = source.publish_request();
    // publish two versions without moving anything, so the alias below is the only pointer that moves.
    request.alias = None;
    for _ in 0..2 {
        let _ = crate::handlers::functions::publish_function::<SqliteDb>(
            Extension(db.clone()),
            function_packages(&db, &blobs),
            Extension(admin_ctx()),
            ValidatedJson(request.clone()),
        )
        .await;
    }

    let package = db
        .fetch_function_package(None, Some(PACKAGE_NAMESPACE), "image-tools")
        .await
        .unwrap()
        .expect("package");

    // point production at version 1, then move it to 2: version 1 is untouched by the move.
    crate::repository::functions::set_alias(
        db.as_ref(),
        package.id,
        "production",
        &FunctionVersionRef::Exact(1),
    )
    .await
    .unwrap();
    let moved = crate::repository::functions::set_alias(
        db.as_ref(),
        package.id,
        "production",
        &FunctionVersionRef::Exact(2),
    )
    .await
    .unwrap();
    assert_eq!(moved.version, 2);

    let one = crate::repository::functions::resolve_version(
        db.as_ref(),
        package.id,
        &FunctionVersionRef::Exact(1),
    )
    .await
    .unwrap();
    assert_eq!(one.version, 1);
    assert_eq!(one.artifact_digest, source.archive.digest);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn publishing_requires_the_functions_capability() {
    // The gate is a capability rather than a bare admin check, so the backend and UI reference
    // one dictionary; a plain member holds none of it.
    let member = AuthContext {
        principal_id: Some(Uuid::new_v4()),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    assert!(
        member
            .require_scope_action(
                runinator_models::rbac::Action::FunctionsManage,
                member.selected_scope()
            )
            .is_err()
    );
    assert!(
        admin_ctx()
            .require_scope_action(
                runinator_models::rbac::Action::FunctionsManage,
                runinator_models::rbac::ScopeRef::PLATFORM
            )
            .is_ok()
    );
}

#[tokio::test]
async fn publishing_generates_a_hidden_adapter_workflow_per_export() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("adapters").await;
    let source = FunctionSource::load(&root).unwrap();

    let _ = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(source.archive.digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    let (status, body) = crate::handlers::functions::publish_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        ValidatedJson(source.publish_request()),
    )
    .await;
    if status != StatusCode::OK
        && let ApiResponse::ApiError(error) = body.0
    {
        panic!("publish failed: {}", error.message);
    }
    assert_eq!(status, StatusCode::OK);

    // one adapter per export, each recorded so the invocation path can find it.
    let package = db
        .fetch_function_package(None, Some(PACKAGE_NAMESPACE), "image-tools")
        .await
        .unwrap()
        .expect("package");
    let versions = db.fetch_function_versions(package.id).await.unwrap();
    let exports = db.fetch_function_exports(versions[0].id).await.unwrap();
    assert_eq!(exports.len(), 2);
    for export in &exports {
        let adapter = db
            .fetch_function_adapter_workflow(export.id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("no adapter for export '{}'", export.name));
        let workflow = crate::repository::fetch_workflow(db.as_ref(), adapter.workflow_id)
            .await
            .unwrap()
            .expect("adapter workflow");
        assert!(crate::repository::function_adapters::is_adapter_workflow(
            &workflow
        ));
    }

    // and they stay out of the workflow list: one entry per published export in a list nobody
    // authored would make the workflows view unusable for anyone with a few packages.
    let listed = crate::repository::fetch_workflows(db.as_ref())
        .await
        .unwrap();
    assert!(
        listed.is_empty(),
        "adapters must not appear in the workflow list, got {:?}",
        listed.iter().map(|w| w.name.clone()).collect::<Vec<_>>()
    );
    // they are still reachable when explicitly asked for, which is what the invocation path needs.
    let all = crate::repository::fetch_workflows_with_managed(db.as_ref(), true)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn republishing_retains_an_immutable_adapter_for_each_version() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("republish").await;
    let source = FunctionSource::load(&root).unwrap();

    let _ = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(source.archive.digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    for _ in 0..2 {
        let (status, _) = crate::handlers::functions::publish_function::<SqliteDb>(
            Extension(db.clone()),
            function_packages(&db, &blobs),
            Extension(admin_ctx()),
            ValidatedJson(source.publish_request()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // latest/alias/exact resolution selects an export first, so every release retains its own
    // adapter and an exact-version request cannot drift after another publish.
    let all = crate::repository::fetch_workflows_with_managed(db.as_ref(), true)
        .await
        .unwrap();
    let mut versions = all
        .iter()
        .filter_map(|workflow| {
            workflow.definition.nodes.iter().find_map(|node| {
                node.action
                    .as_ref()
                    .and_then(|action| action.function_binding.as_ref())
                    .map(|binding| binding.version)
            })
        })
        .collect::<Vec<_>>();
    versions.sort_unstable();
    assert_eq!(versions, vec![1, 1, 2, 2]);
    assert_eq!(all.len(), 4);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn deleting_a_package_archives_it_and_restore_reactivates_it() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("archive-restore").await;
    let _ = published(&db, &blobs, &root).await;

    let (status, _) = crate::handlers::functions::delete_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(PACKAGE_PATH.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let package = db
        .fetch_function_package(None, Some(PACKAGE_NAMESPACE), "image-tools")
        .await
        .unwrap()
        .expect("archived package remains");
    assert!(package.archived_at.is_some());

    let (status, _) = crate::handlers::functions::get_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(PACKAGE_PATH.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = crate::handlers::functions::restore_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(&db, &blobs),
        Extension(admin_ctx()),
        Path(PACKAGE_PATH.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let restored = db
        .fetch_function_package_by_id(package.id)
        .await
        .unwrap()
        .expect("restored package");
    assert!(restored.archived_at.is_none());
    assert_eq!(db.fetch_function_catalog().await.unwrap().len(), 2);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

// an event bus wired to an in-memory broker: the handlers emit through it, and nothing in these
// tests reads what comes out.
fn test_event_bus() -> crate::events::EventSender {
    let (tx, _rx) = tokio::sync::broadcast::channel(64);
    crate::events::EventBus::new(tx, Arc::new(InMemoryBroker::new()))
}

// publish the fixture package and return its two adapter workflow ids by export name.
async fn published(
    db: &Arc<SqliteDb>,
    blobs: &Arc<dyn BlobStore>,
    root: &std::path::Path,
) -> std::collections::BTreeMap<String, Uuid> {
    let source = FunctionSource::load(root).unwrap();
    let _ = crate::handlers::functions::upload_function_artifact::<SqliteDb>(
        Extension(db.clone()),
        function_packages(db, blobs),
        Extension(admin_ctx()),
        Path(source.archive.digest.clone()),
        source.archive.bytes.clone().into(),
    )
    .await;
    let (status, _) = crate::handlers::functions::publish_function::<SqliteDb>(
        Extension(db.clone()),
        function_packages(db, blobs),
        Extension(admin_ctx()),
        ValidatedJson(source.publish_request()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let package = db
        .fetch_function_package(None, Some(PACKAGE_NAMESPACE), "image-tools")
        .await
        .unwrap()
        .expect("package");
    let versions = db.fetch_function_versions(package.id).await.unwrap();
    let mut adapters = std::collections::BTreeMap::new();
    for export in db.fetch_function_exports(versions[0].id).await.unwrap() {
        let adapter = db
            .fetch_function_adapter_workflow(export.id)
            .await
            .unwrap()
            .expect("adapter");
        adapters.insert(export.name, adapter.workflow_id);
    }
    adapters
}

#[tokio::test]
async fn an_http_invocation_starts_a_run_of_the_adapter_workflow() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("invoke").await;
    let adapters = published(&db, &blobs, &root).await;
    let events = test_event_bus();

    let (status, body) =
        crate::handlers::function_invocations::create_function_invocation::<SqliteDb>(
            Extension(db.clone()),
            function_invocations(&db, &events),
            Extension(admin_ctx()),
            axum::http::HeaderMap::from_iter([(
                axum::http::HeaderName::from_static("prefer"),
                axum::http::HeaderValue::from_static("respond-async"),
            )]),
            Path((PACKAGE_PATH.to_string(), "resize".to_string())),
            axum::extract::Query(Default::default()),
            Json(json!({ "source": "a.png", "width": 320 })),
        )
        .await;

    // `Prefer: respond-async` skips the bounded wait, so this is deterministic without a worker.
    assert_eq!(status, StatusCode::ACCEPTED);
    let run = match body.0 {
        ApiResponse::WorkflowRun(run) => run,
        _ => panic!("unexpected response"),
    };
    // the run is of the *adapter*, which is what makes http and rexrap invocation the same machinery.
    assert_eq!(Some(run.run.workflow_id), adapters.get("resize").copied());
    assert_eq!(
        run.run.parameters.get("source").and_then(Value::as_str),
        Some("a.png")
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn an_idempotency_key_replays_the_run_it_already_started() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("idempotent").await;
    let adapters = published(&db, &blobs, &root).await;
    let events = test_event_bus();

    let headers = axum::http::HeaderMap::from_iter([
        (
            axum::http::HeaderName::from_static("prefer"),
            axum::http::HeaderValue::from_static("respond-async"),
        ),
        (
            axum::http::HeaderName::from_static("idempotency-key"),
            axum::http::HeaderValue::from_static("abc-123"),
        ),
    ]);
    let invoke = async |headers: axum::http::HeaderMap| {
        crate::handlers::function_invocations::create_function_invocation::<SqliteDb>(
            Extension(db.clone()),
            function_invocations(&db, &events),
            Extension(admin_ctx()),
            headers,
            Path((PACKAGE_PATH.to_string(), "resize".to_string())),
            axum::extract::Query(Default::default()),
            Json(json!({ "source": "a.png" })),
        )
        .await
    };

    let first = match invoke(headers.clone()).await.1.0 {
        ApiResponse::WorkflowRun(run) => run.run.id,
        _ => panic!("unexpected response"),
    };
    let second = match invoke(headers).await.1.0 {
        ApiResponse::WorkflowRun(run) => run.run.id,
        _ => panic!("unexpected response"),
    };
    // a retried request must not start a second execution of the same work.
    assert_eq!(first, second);
    let runs = crate::repository::fetch_workflow_runs_for_workflow(db.as_ref(), adapters["resize"])
        .await
        .unwrap();
    assert_eq!(
        runs.len(),
        1,
        "the key must replay rather than start a second run"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn invoking_an_unknown_export_is_not_found() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("unknown").await;
    published(&db, &blobs, &root).await;
    let events = test_event_bus();

    let (status, _) =
        crate::handlers::function_invocations::create_function_invocation::<SqliteDb>(
            Extension(db.clone()),
            function_invocations(&db, &events),
            Extension(admin_ctx()),
            axum::http::HeaderMap::new(),
            Path((PACKAGE_PATH.to_string(), "crop".to_string())),
            axum::extract::Query(Default::default()),
            Json(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn invoking_is_gated_separately_from_publishing() {
    use runinator_ws_middleware::authz::AuthContextExt;

    // publishing and calling are different privileges: a service account that runs a function
    // should not be able to replace the code it runs. an admin holds both; neither implies the
    // other for anyone else.
    let member = AuthContext {
        principal_id: Some(Uuid::new_v4()),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    assert!(
        member
            .require_scope_action(runinator_models::rbac::Action::Run, member.selected_scope())
            .is_err()
    );
    assert!(
        member
            .require_scope_action(
                runinator_models::rbac::Action::FunctionsManage,
                member.selected_scope()
            )
            .is_err()
    );
    assert!(
        admin_ctx()
            .require_scope_action(
                runinator_models::rbac::Action::Run,
                runinator_models::rbac::ScopeRef::PLATFORM
            )
            .is_ok()
    );

    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (root, blobs, blob_root) = fixture("invoke-gate").await;
    published(&db, &blobs, &root).await;

    let (status, _) =
        crate::handlers::function_invocations::create_function_invocation::<SqliteDb>(
            Extension(db.clone()),
            function_invocations(&db, &test_event_bus()),
            Extension(member),
            axum::http::HeaderMap::new(),
            Path((PACKAGE_PATH.to_string(), "resize".to_string())),
            axum::extract::Query(Default::default()),
            Json(json!({ "source": "a.png" })),
        )
        .await;
    // An inaccessible ID-addressed resource is deliberately concealed as not found.
    assert_eq!(status, StatusCode::NOT_FOUND);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&blob_root);
    let _ = std::fs::remove_file(db_path);
}
