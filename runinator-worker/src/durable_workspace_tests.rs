//! Durable workspace lifecycle phase reporting.
use super::*;

fn workspace_http_error(message: &str) -> ApiError {
    ApiError::Http {
        status: reqwest::StatusCode::CONFLICT,
        url: reqwest::Url::parse("http://runinator.test/workspaces/checkouts/test/content")
            .unwrap(),
        message: message.into(),
    }
}

#[test]
fn completed_phases_are_bounded_summary_records() {
    let reporter = WorkspacePhaseReporter::default();
    reporter
        .start("workspace.restore.index")
        .succeeded(runinator_models::json!({"objects": 52_000, "packs": 1}));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].phase, "workspace.restore.index");
    assert_eq!(events[0].status, "succeeded");
    assert_eq!(events[0].details["objects"], 52_000);
    assert!(reporter.drain().is_empty());
}

#[test]
fn an_unfinished_phase_records_one_failure() {
    let reporter = WorkspacePhaseReporter::default();
    drop(reporter.start("workspace.snapshot.capture"));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status, "failed");
}

#[tokio::test]
async fn restore_retries_until_the_workspace_claim_is_visible() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let bytes = retry_pending_workspace_claim(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        |_| {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(if call == 0 {
                Err(workspace_http_error(
                    "workspace.conflict: WORKSPACE002 - Workspace version or checkout is no longer current: replica has not claimed this active attempt",
                ))
            } else {
                Ok(vec![1, 2, 3])
            })
        },
    )
    .await
    .unwrap();

    assert_eq!(bytes, vec![1, 2, 3]);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn restore_does_not_retry_a_genuinely_stale_checkout() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let error = retry_pending_workspace_claim(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        |_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Err(workspace_http_error(
                "workspace.conflict: WORKSPACE002 - Workspace version or checkout is no longer current: checkout is no longer active",
            )))
        },
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("checkout is no longer active"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

struct CheckoutSource {
    checkout: uuid::Uuid,
    replica: uuid::Uuid,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl WorkspaceCheckoutClient for CheckoutSource {
    async fn download_workspace_checkout(
        &self,
        checkout: uuid::Uuid,
        replica: uuid::Uuid,
        timeout: std::time::Duration,
    ) -> runinator_api::Result<Vec<u8>> {
        assert_eq!((checkout, replica), (self.checkout, self.replica));
        assert!(!timeout.is_zero());
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(workspace_http_error(
                "replica has not claimed this active attempt",
            ));
        }
        Ok(vec![7, 8])
    }
    async fn seal_workspace(
        &self,
        _: uuid::Uuid,
        _: uuid::Uuid,
        _: String,
        _: std::time::Duration,
    ) -> runinator_api::Result<WorkspaceReceipt> {
        panic!("restore must not seal a checkout")
    }
}

#[tokio::test]
async fn injected_checkout_transport_preserves_claim_retry_and_scope() {
    let source = CheckoutSource {
        checkout: uuid::Uuid::new_v4(),
        replica: uuid::Uuid::new_v4(),
        calls: Default::default(),
    };
    let bytes = download_workspace_checkout_after_claim(
        &source,
        source.checkout,
        source.replica,
        std::time::Instant::now() + std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(bytes, vec![7, 8]);
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}
