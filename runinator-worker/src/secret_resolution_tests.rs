//! Injected secret reads retain run scope and failure classification.
use super::*;

struct Secrets {
    id: Uuid,
    run: Uuid,
    fail: bool,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl RunSecretReader for Secrets {
    async fn fetch_credential_by_id_for_run(
        &self,
        id: Uuid,
        run: Uuid,
    ) -> runinator_api::Result<String> {
        assert_eq!((id, run), (self.id, self.run));
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail {
            return Err(ApiError::CircuitOpen {
                target: "test".into(),
                retry_after_seconds: 30,
            });
        }
        Ok("resolved".into())
    }
}

#[tokio::test]
async fn repeated_references_are_read_once_and_transient_failures_remain_transient() {
    let id = Uuid::new_v4();
    let run = Uuid::new_v4();
    let reference = format!("secret+uuid://{id}/acme.shared/token");
    for fail in [false, true] {
        let source = Secrets {
            id,
            run,
            fail,
            calls: Default::default(),
        };
        let result = resolve_secret_refs(
            &source,
            run,
            runinator_models::json!({"a": reference, "b": [reference]}),
        )
        .await;
        assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        if fail {
            assert!(is_transient_secret_error(&result.unwrap_err()));
        } else {
            assert_eq!(
                result.unwrap(),
                runinator_models::json!({"a": "resolved", "b": ["resolved"]})
            );
        }
    }
}
