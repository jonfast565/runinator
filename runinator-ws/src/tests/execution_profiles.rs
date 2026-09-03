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
