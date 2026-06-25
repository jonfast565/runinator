use axum::body::to_bytes;
use axum::http::StatusCode;

use super::handle_panic;

// the various payload types `panic!`/`assert!` produce should all map to a 500 without the panic
// handler itself panicking on an unexpected payload type, and the body must be the generic envelope
// so panic internals never reach the client.
#[tokio::test]
async fn handle_panic_returns_internal_error_envelope() {
    for payload in [
        Box::new("boom") as Box<dyn std::any::Any + Send>,
        Box::new(String::from("boom")),
        Box::new(42u32),
    ] {
        let response = handle_panic(payload);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["message"], "internal server error");
    }
}
