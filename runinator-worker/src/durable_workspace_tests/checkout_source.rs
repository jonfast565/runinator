#[allow(unused_imports)]
use super::*;

pub(super) struct CheckoutSource {
    pub(super) checkout: uuid::Uuid,
    pub(super) replica: uuid::Uuid,
    pub(super) calls: std::sync::atomic::AtomicUsize,
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
