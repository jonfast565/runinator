//! execution-profile routes and authorized worker consumption.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode},
};
use runinator_engine::services::ExecutionProfileOperations;
use runinator_models::rbac::SystemRole;
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn routes_receive_the_database_extension() {
    let (db, _path) = test_db().await;
    let db = Arc::new(db);
    let service = Arc::new(ExecutionProfileOperations::new(db.clone()));
    let context = AuthContext {
        system_role: Some(SystemRole::Agent),
        ..user_ctx(Uuid::now_v7())
    };
    let app = crate::handlers::execution_profiles::routes(db)
        .layer(Extension(service))
        .layer(Extension(context));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/execution_profiles")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn profile_download_honors_worker_authority_and_run_admission() {
    use axum::{body::to_bytes, extract::Query};
    use runinator_blob::{BlobStore, FsBlobStore, sha256_hex};
    use runinator_models::{
        execution_profiles::ExecutionProfilePublishRequest, rbac::PlatformRole,
    };

    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let service = Arc::new(ExecutionProfileOperations::new(db.clone()));
    let root = std::env::temp_dir().join(format!("runinator-profile-download-{}", Uuid::new_v4()));
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(&root).await.unwrap());
    let profile_id = Uuid::new_v4();
    service
        .configure(
            profile_id,
            None,
            serde_json::from_value(serde_json::json!({
                "name": "claude", "credential_scopes": ["claude"],
                "collection": { "sources": [
                    { "type": "file", "path": "~/.claude/config", "target": ".claude/config" }
                ] }
            }))
            .unwrap(),
            None,
            true,
        )
        .await
        .unwrap();
    let context = AuthContext {
        kind: PrincipalKind::Service,
        platform_role: Some(PlatformRole::Admin),
        ..user_ctx(Uuid::new_v4())
    };
    let bundle = b"test execution profile bundle";
    let bundle_digest = sha256_hex(bundle);
    let (status, _) = crate::handlers::execution_profiles::publish(
        Extension(db.clone()),
        Extension(service.clone()),
        Extension(blobs.clone()),
        Extension(context.clone()),
        Path(profile_id),
        Query(ExecutionProfilePublishRequest {
            digest: bundle_digest.clone(),
            expires_at: None,
        }),
        bundle.as_slice().into(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let stored = service
        .fetch_revision(profile_id, 1)
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored
            .uri
            .ends_with(&format!("/{profile_id}/{bundle_digest}.bundle"))
    );
    for provider in runinator_provider_catalog::metadata() {
        crate::repository::upsert_catalog_item(
            db.as_ref(),
            crate::provider_catalog_item(&provider),
        )
        .await
        .unwrap();
    }
    let mut definition = workflow(None, "Profile consumer");
    definition.definition = WorkflowGraph::from_value(json!({
        "start": "probe", "nodes": [
            { "id": "probe", "kind": "action", "action": {
                "provider": "ai-command", "function": "claude_code",
                "configuration": { "prompt": "Reply yes" },
                "execution_profile": { "reference": { "kind": "execution_profile", "id": profile_id } }
            }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    })).unwrap();
    let definition = save_workflow(db.as_ref(), &definition).await.unwrap();
    let run = crate::repository::create_workflow_run(
        db.as_ref(),
        definition.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    for (role, admin, admitted, org_id, revision, ceiling, expected) in [
        (
            Some(SystemRole::Worker),
            false,
            true,
            None,
            1,
            vec![],
            StatusCode::OK,
        ),
        (None, true, true, None, 1, vec![], StatusCode::OK),
        (None, false, true, None, 1, vec![], StatusCode::NOT_FOUND),
        // a desktop relay hosts a worker under the agent role, so an admitted agent consumer is
        // served the bundle, and an unadmitted one is still refused.
        (
            Some(SystemRole::Agent),
            false,
            true,
            None,
            1,
            vec![],
            StatusCode::OK,
        ),
        (
            Some(SystemRole::Agent),
            false,
            false,
            None,
            1,
            vec![],
            StatusCode::NOT_FOUND,
        ),
        (
            Some(SystemRole::Engine),
            false,
            true,
            None,
            1,
            vec![],
            StatusCode::NOT_FOUND,
        ),
        (None, true, false, None, 1, vec![], StatusCode::NOT_FOUND),
        (
            Some(SystemRole::Worker),
            false,
            false,
            None,
            1,
            vec![],
            StatusCode::NOT_FOUND,
        ),
        // this fixture profile is platform scoped, so an active organization selection does not
        // hide it. cross-organization isolation is covered by the org-scoped profile test below.
        (
            None,
            true,
            true,
            Some(Uuid::new_v4()),
            1,
            vec![],
            StatusCode::OK,
        ),
        (None, true, true, None, 2, vec![], StatusCode::NOT_FOUND),
        (
            None,
            true,
            true,
            None,
            1,
            vec![runinator_models::rbac::Action::View],
            StatusCode::NOT_FOUND,
        ),
    ] {
        let response = crate::handlers::execution_profiles::content(
            Extension(db.clone()),
            Extension(service.clone()),
            Extension(blobs.clone()),
            Extension(AuthContext {
                system_role: role,
                platform_role: admin.then_some(PlatformRole::Admin),
                org_id,
                action_ceiling: ceiling,
                ..context.clone()
            }),
            Path((profile_id, revision)),
            Query(crate::handlers::execution_profiles::ProfileLookup {
                name: None,
                consumer_run_id: admitted.then_some(run.id),
                consumer_adapter_dispatch_id: None,
            }),
        )
        .await;
        assert_eq!(
            response.status(),
            expected,
            "role={role:?}, admin={admin}, admitted={admitted}, org={org_id:?}, revision={revision}"
        );
        if expected == StatusCode::OK {
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                bundle
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn org_scoped_profile_bundle_stays_invisible_to_another_organization() {
    use axum::extract::Query;
    use runinator_blob::{BlobStore, FsBlobStore, sha256_hex};
    use runinator_models::{
        execution_profiles::ExecutionProfilePublishRequest, rbac::PlatformRole,
    };

    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let service = Arc::new(ExecutionProfileOperations::new(db.clone()));
    let root = std::env::temp_dir().join(format!("runinator-profile-tenancy-{}", Uuid::new_v4()));
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(&root).await.unwrap());
    let owning_org = Uuid::new_v4();
    let profile_id = Uuid::new_v4();
    service
        .configure(
            profile_id,
            Some(owning_org),
            serde_json::from_value(serde_json::json!({
                "name": "claude", "credential_scopes": ["claude"],
                "collection": { "sources": [
                    { "type": "file", "path": "~/.claude/config", "target": ".claude/config" }
                ] }
            }))
            .unwrap(),
            None,
            true,
        )
        .await
        .unwrap();
    let context = AuthContext {
        kind: PrincipalKind::Service,
        platform_role: Some(PlatformRole::Admin),
        ..user_ctx(Uuid::new_v4())
    };
    // publishing is a write path and stays strictly org matched, so the bundle is published from
    // inside the owning organization.
    let owner_context = AuthContext {
        org_id: Some(owning_org),
        ..context.clone()
    };
    let bundle = b"org scoped execution profile bundle";
    let bundle_digest = sha256_hex(bundle);
    let (status, _) = crate::handlers::execution_profiles::publish(
        Extension(db.clone()),
        Extension(service.clone()),
        Extension(blobs.clone()),
        Extension(owner_context),
        Path(profile_id),
        Query(ExecutionProfilePublishRequest {
            digest: bundle_digest,
            expires_at: None,
        }),
        bundle.as_slice().into(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for provider in runinator_provider_catalog::metadata() {
        crate::repository::upsert_catalog_item(
            db.as_ref(),
            crate::provider_catalog_item(&provider),
        )
        .await
        .unwrap();
    }
    let mut definition = workflow(None, "Org scoped profile consumer");
    definition.org_id = Some(owning_org);
    definition.definition = WorkflowGraph::from_value(json!({
        "start": "probe", "nodes": [
            { "id": "probe", "kind": "action", "action": {
                "provider": "ai-command", "function": "claude_code",
                "configuration": { "prompt": "Reply yes" },
                "execution_profile": { "reference": { "kind": "execution_profile", "id": profile_id } }
            }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    })).unwrap();
    let definition = save_workflow(db.as_ref(), &definition).await.unwrap();
    let run = crate::repository::create_workflow_run(
        db.as_ref(),
        definition.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    // the run admits this profile in both cases, so the only thing separating them is tenancy.
    for (org_id, expected) in [
        (Some(owning_org), StatusCode::OK),
        (Some(Uuid::new_v4()), StatusCode::NOT_FOUND),
        (None, StatusCode::NOT_FOUND),
    ] {
        let response = crate::handlers::execution_profiles::content(
            Extension(db.clone()),
            Extension(service.clone()),
            Extension(blobs.clone()),
            Extension(AuthContext {
                system_role: Some(SystemRole::Worker),
                org_id,
                ..context.clone()
            }),
            Path((profile_id, 1)),
            Query(crate::handlers::execution_profiles::ProfileLookup {
                name: None,
                consumer_run_id: Some(run.id),
                consumer_adapter_dispatch_id: None,
            }),
        )
        .await;
        assert_eq!(response.status(), expected, "org={org_id:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
    let _ = std::fs::remove_file(path);
}
