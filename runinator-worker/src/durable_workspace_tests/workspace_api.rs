#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct WorkspaceApi {
    pub(super) checkout: WorkspaceCheckout,
    pub(super) archive: Arc<Vec<u8>>,
    pub(super) objects: Arc<HashMap<String, Vec<u8>>>,
    pub(super) downloads: Arc<std::sync::atomic::AtomicUsize>,
    pub(super) reads: Arc<std::sync::Mutex<Vec<String>>>,
    pub(super) uploads: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
}

#[async_trait::async_trait]
impl WorkspaceCheckoutClient for WorkspaceApi {
    async fn download_workspace_checkout(
        &self,
        checkout: uuid::Uuid,
        _: uuid::Uuid,
        _: std::time::Duration,
    ) -> runinator_api::Result<Vec<u8>> {
        assert_eq!(checkout, self.checkout.id);
        self.downloads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.archive.as_ref().clone())
    }

    async fn seal_workspace(
        &self,
        checkout: uuid::Uuid,
        _: uuid::Uuid,
        revision_id: String,
        _: std::time::Duration,
    ) -> runinator_api::Result<WorkspaceReceipt> {
        assert_eq!(checkout, self.checkout.id);
        Ok(WorkspaceReceipt {
            id: uuid::Uuid::new_v4(),
            checkout: self.checkout.clone(),
            snapshot: WorkspaceSnapshot {
                workspace_id: self.checkout.workspace_id,
                version: self.checkout.base_version + 1,
                parent_version: self.checkout.base_version,
                origin: WorkspaceOrigin::Workflow {
                    workflow_run_id: self.checkout.workflow_run_id,
                    effect_id: self.checkout.effect_id,
                    attempt: self.checkout.attempt,
                },
                revision_id,
                usage: WorkspaceUsage::default(),
                limits: self.checkout.limits,
                created_at: chrono::Utc::now(),
            },
        })
    }
}

#[async_trait::async_trait]
impl WorkspaceObjectTransport for WorkspaceApi {
    async fn workspace_object(
        &self,
        checkout: uuid::Uuid,
        _: uuid::Uuid,
        id: &str,
    ) -> runinator_api::Result<Option<Vec<u8>>> {
        assert_eq!(checkout, self.checkout.id);
        self.reads.lock().unwrap().push(id.into());
        Ok(self.objects.get(id).cloned())
    }

    async fn upload_workspace_pack(
        &self,
        checkout: uuid::Uuid,
        _: uuid::Uuid,
        bytes: Vec<u8>,
    ) -> runinator_api::Result<()> {
        assert_eq!(checkout, self.checkout.id);
        self.uploads.lock().unwrap().push(bytes);
        Ok(())
    }
}
