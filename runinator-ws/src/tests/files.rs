//! Workflow-file routes receive both their database and application-service extensions.

use super::*;
use axum::{Extension, body::Body, http::Request};
use runinator_blob::FsBlobStore;
use runinator_engine::services::WorkflowFiles;
use runinator_models::auth::AuthContext;
use tower::ServiceExt;

#[tokio::test]
async fn workflow_file_library_route_receives_database_extension() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let blob_root = std::env::temp_dir().join(format!("workflow-files-{}", Uuid::now_v7()));
    let blobs = Arc::new(FsBlobStore::open(&blob_root).await.unwrap());
    let files = Arc::new(WorkflowFiles::new(db.clone(), blobs));
    let app = crate::handlers::files::routes(db.clone())
        .layer(Extension(files))
        .layer(Extension(AuthContext::disabled_platform_admin()));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/workflow_files")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    drop(db);
    let _ = std::fs::remove_dir_all(blob_root);
    let _ = std::fs::remove_file(path);
}
