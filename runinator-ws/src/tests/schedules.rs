use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode, header},
};
use runinator_broker::UiEventPublisher;
use runinator_engine::services::SchedulingOperations;
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn calendar_routes_receive_the_database_extension() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let service = Arc::new(SchedulingOperations::new(
        db.clone(),
        UiEventPublisher::new(Arc::new(InMemoryBroker::new())),
        None,
    ));
    let app = crate::handlers::schedules::routes(db)
        .layer(Extension(service))
        .layer(Extension(user_ctx(Uuid::now_v7())));

    let download = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/schedules/calendar.ics?scope=user")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static(
            "text/calendar; charset=utf-8"
        ))
    );

    let subscription = app
        .oneshot(
            Request::builder()
                .uri("/calendar/unknown/runinator.ics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(subscription.status(), StatusCode::NOT_FOUND);

    let _ = std::fs::remove_file(path);
}
