//! Workspace management enforces stored resource permissions.
use super::*;
use axum::response::IntoResponse;
use runinator_blob::FsBlobStore;
use runinator_engine::services::WorkspaceService;
use runinator_models::{
    rbac::{ResourceOwnership, ScopeRef},
    workspaces::DurableWorkspace,
};

#[tokio::test]
async fn workspace_management_requires_view_and_ownership() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let root = std::env::temp_dir().join(format!("workspace-auth-{}", Uuid::now_v7()));
    let blobs = Arc::new(FsBlobStore::open(&root).await.unwrap());
    let service = Arc::new(WorkspaceService::new(db.clone(), blobs));
    let now = chrono::Utc::now();
    let id = Uuid::now_v7();
    service
        .create(
            DurableWorkspace {
                id,
                key: "permissions".into(),
                org_id: None,
                head_version: 0,
                revision: 1,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            },
            ResourceOwnership {
                resource_type: ResourceType::Workspace,
                resource_id: id,
                tenant: ScopeRef::PLATFORM,
                owner: ScopeRef::PLATFORM,
                created_by: None,
                authz_version: 1,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
    let reader = db
        .create_user("workspace-reader".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    let stranger = db
        .create_user("workspace-stranger".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    let mut permission = grant(id, PrincipalType::User, reader, Permission::View);
    permission.resource_type = ResourceType::Workspace;
    db.create_grant(permission).await.unwrap();
    let read = crate::handlers::workspaces::detail::<SqliteDb>(
        Extension(db.clone()),
        Extension(service.clone()),
        Extension(user_ctx(reader)),
        Path(id),
    )
    .await;
    assert_eq!(read.status(), StatusCode::OK);
    let hidden = crate::handlers::workspaces::detail::<SqliteDb>(
        Extension(db.clone()),
        Extension(service.clone()),
        Extension(user_ctx(stranger)),
        Path(id),
    )
    .await;
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    let denied = crate::handlers::workspaces::remove::<SqliteDb>(
        Extension(db.clone()),
        Extension(service.clone()),
        Extension(user_ctx(reader)),
        Path(id),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let mut admin = user_ctx(reader);
    admin.platform_role = Some(runinator_models::rbac::PlatformRole::Admin);
    let removed = crate::handlers::workspaces::remove::<SqliteDb>(
        Extension(db.clone()),
        Extension(service),
        Extension(admin),
        Path(id),
    )
    .await
    .into_response();
    assert_eq!(removed.status(), StatusCode::NO_CONTENT);
    drop(db);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(path);
}
