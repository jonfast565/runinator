use super::*;

// provider delegation and error propagation through WorkspaceService.

use runinator_database::sqlite::SqliteDb;
use runinator_models::errors::WORKSPACE_INVALID;
use runinator_store::DatabaseImpl;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct RecordingProvider {
    calls: Mutex<Vec<String>>,
    fail_file_range: AtomicBool,
}

impl RecordingProvider {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail_file_range: AtomicBool::new(false),
        }
    }

    fn record(&self, call: impl Into<String>) {
        self.calls.lock().unwrap().push(call.into());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

fn checkout() -> WorkspaceCheckout {
    WorkspaceCheckout {
        limits: WorkspaceLimits::default(),
        id: uuid::Uuid::from_u128(1),
        workspace_id: uuid::Uuid::from_u128(2),
        workflow_run_id: uuid::Uuid::from_u128(3),
        effect_id: uuid::Uuid::from_u128(4),
        attempt: 5,
        base_version: 6,
        access: WorkspaceAccess::Write,
        fence: 7,
        leased_until: chrono::Utc::now() + chrono::Duration::minutes(5),
    }
}

fn snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        workspace_id: uuid::Uuid::from_u128(2),
        version: 7,
        parent_version: 6,
        origin: WorkspaceOrigin::Workflow {
            workflow_run_id: uuid::Uuid::from_u128(3),
            effect_id: uuid::Uuid::from_u128(4),
            attempt: 5,
        },
        revision_id: "a".repeat(64),
        usage: WorkspaceUsage::default(),
        limits: WorkspaceLimits::default(),
        created_at: chrono::Utc::now(),
    }
}

fn receipt() -> WorkspaceReceipt {
    WorkspaceReceipt {
        id: uuid::Uuid::from_u128(8),
        checkout: checkout(),
        snapshot: snapshot(),
    }
}

fn content(size_bytes: u64) -> WorkspaceContent {
    WorkspaceContent {
        size_bytes,
        sha256: None,
        body: Box::new(tokio::io::empty()),
    }
}

fn transfer() -> WorkspaceTransfer {
    WorkspaceTransfer {
        filesystem: false,
        id: uuid::Uuid::from_u128(9),
        workspace_id: uuid::Uuid::from_u128(2),
        version: 7,
        importing: false,
        state: "running".into(),
        bytes_processed: 0,
        error: None,
        limits: WorkspaceLimits::default(),
        created_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::days(1),
        token: uuid::Uuid::from_u128(10),
        archive_uri: None,
    }
}

#[async_trait]
impl WorkspaceStorageProvider<SqliteDb> for RecordingProvider {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn configure_limits(&self, _limits: WorkspaceLimits) {}

    async fn checkout_content(
        &self,
        checkout: WorkspaceCheckout,
    ) -> Result<WorkspaceContent, SendableError> {
        self.record(format!("checkout_content:{}", checkout.id));
        Ok(content(11))
    }

    async fn object(
        &self,
        workspace: uuid::Uuid,
        id: String,
        version: i64,
    ) -> Result<Vec<u8>, SendableError> {
        self.record(format!("object:{workspace}:{id}:{version}"));
        Ok(b"object".to_vec())
    }

    async fn stage_checkout(&self, id: uuid::Uuid, bytes: Vec<u8>) -> Result<(), SendableError> {
        self.record(format!("stage_checkout:{id}:{}", bytes.len()));
        Ok(())
    }

    async fn seal_checkout(
        &self,
        id: uuid::Uuid,
        request: WorkspaceSeal,
    ) -> Result<WorkspaceReceipt, SendableError> {
        self.record(format!("seal_checkout:{id}:{}", request.revision_id));
        Ok(receipt())
    }

    async fn directory(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        self.record(format!("directory:{id}:{version}:{path}:{after:?}:{limit}"));
        Ok(WorkspaceDirectory {
            revision_id: "directory".into(),
            path,
            entries: Vec::new(),
            next_cursor: None,
        })
    }

    async fn results(
        &self,
        id: uuid::Uuid,
        version: i64,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        self.record(format!("results:{id}:{version}:{after:?}:{limit}"));
        Ok(WorkspaceDirectory {
            revision_id: "results".into(),
            path: String::new(),
            entries: Vec::new(),
            next_cursor: None,
        })
    }

    async fn result_content(
        &self,
        id: uuid::Uuid,
        version: i64,
        name: String,
        preview: bool,
    ) -> Result<WorkspaceContent, SendableError> {
        self.record(format!("result_content:{id}:{version}:{name}:{preview}"));
        Ok(content(12))
    }

    async fn file_range(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, SendableError> {
        self.record(format!(
            "file_range:{id}:{version}:{path}:{offset}:{length}"
        ));
        if self.fail_file_range.load(Ordering::Acquire) {
            return Err(WORKSPACE_INVALID.error("recording provider failure"));
        }
        Ok(b"range".to_vec())
    }

    async fn file(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
    ) -> Result<WorkspaceContent, SendableError> {
        self.record(format!("file:{id}:{version}:{path}"));
        Ok(content(13))
    }

    async fn diff(
        &self,
        id: uuid::Uuid,
        before: i64,
        after: i64,
        cursor: Option<String>,
    ) -> Result<WorkspaceDiff, SendableError> {
        self.record(format!("diff:{id}:{before}:{after}:{cursor:?}"));
        Ok(WorkspaceDiff {
            before_revision: "before".into(),
            after_revision: "after".into(),
            changes: Vec::new(),
            next_cursor: None,
        })
    }

    async fn process_transfer(
        &self,
        job: &WorkspaceTransfer,
        _progress: &crate::services::workspace_transfers::WorkspaceTransferProgress,
    ) -> Result<(), SendableError> {
        self.record(format!("process_transfer:{}", job.id));
        Ok(())
    }

    async fn collect_workspaces(&self) -> Result<(), SendableError> {
        self.record("collect_workspaces");
        Ok(())
    }

    async fn cleanup_effect_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        self.record(format!("cleanup_effect_orphans:{cursor:?}"));
        Ok(Some("effect-next".into()))
    }

    async fn cleanup_native_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        self.record(format!("cleanup_native_orphans:{cursor:?}"));
        Ok(Some("native-next".into()))
    }

    async fn cleanup_transfer_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        self.record(format!("cleanup_transfer_orphans:{cursor:?}"));
        Ok(Some("transfer-next".into()))
    }

    async fn cleanup_deleted_snapshots(&self) -> Result<(), SendableError> {
        self.record("cleanup_deleted_snapshots");
        Ok(())
    }
}

async fn service(provider: Arc<RecordingProvider>) -> WorkspaceService<SqliteDb> {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.keep().join("delegation.sqlite");
    let db = Arc::new(
        SqliteDb::new(database_path.to_str().unwrap())
            .await
            .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let blobs = Arc::new(
        runinator_blob_core::FsBlobStore::open(database_path.with_extension("blobs"))
            .await
            .unwrap(),
    );
    WorkspaceService::with_storage_provider(db, blobs, WorkspaceLimits::default(), provider)
}

#[tokio::test]
async fn service_delegates_every_storage_operation_once() {
    let provider = Arc::new(RecordingProvider::new());
    let service = service(provider.clone()).await;
    let workspace = uuid::Uuid::from_u128(2);

    assert_eq!(
        service
            .checkout_content(checkout())
            .await
            .unwrap()
            .size_bytes,
        11
    );
    assert_eq!(
        service
            .object(workspace, "object-id".into(), 7)
            .await
            .unwrap(),
        b"object"
    );
    service
        .upload_pack(uuid::Uuid::from_u128(1), vec![1, 2, 3])
        .await
        .unwrap();
    assert_eq!(
        service
            .seal(
                uuid::Uuid::from_u128(1),
                WorkspaceSeal {
                    revision_id: "a".repeat(64),
                },
            )
            .await
            .unwrap()
            .id,
        uuid::Uuid::from_u128(8)
    );
    assert_eq!(
        service
            .directory(workspace, 7, "src".into(), Some("after".into()), 20)
            .await
            .unwrap()
            .revision_id,
        "directory"
    );
    assert_eq!(
        service
            .results(workspace, 7, None, 21)
            .await
            .unwrap()
            .revision_id,
        "results"
    );
    assert_eq!(
        service
            .result_content(workspace, 7, "answer".into(), true)
            .await
            .unwrap()
            .size_bytes,
        12
    );
    assert_eq!(
        service
            .file_range(workspace, 7, "src/lib.rs".into(), 4, 5)
            .await
            .unwrap(),
        b"range"
    );
    assert_eq!(
        service
            .file(workspace, 7, "README.md".into())
            .await
            .unwrap()
            .size_bytes,
        13
    );
    assert_eq!(
        service
            .diff(workspace, 6, 7, Some("cursor".into()))
            .await
            .unwrap()
            .before_revision,
        "before"
    );
    service
        .process_storage_transfer(
            &transfer(),
            &crate::services::workspace_transfers::WorkspaceTransferProgress::testing(),
        )
        .await
        .unwrap();
    service.collect_workspaces().await.unwrap();
    assert_eq!(
        service
            .cleanup_effect_orphans(Some("effect".into()))
            .await
            .unwrap(),
        Some("effect-next".into())
    );
    assert_eq!(
        service
            .cleanup_native_orphans(Some("native".into()))
            .await
            .unwrap(),
        Some("native-next".into())
    );
    assert_eq!(
        service
            .cleanup_transfer_orphans(Some("transfer".into()))
            .await
            .unwrap(),
        Some("transfer-next".into())
    );
    service.cleanup_deleted_snapshots().await.unwrap();

    let calls = provider.calls();
    assert_eq!(calls.len(), 16, "{calls:#?}");
    for expected in [
        "checkout_content:",
        "object:",
        "stage_checkout:",
        "seal_checkout:",
        "directory:",
        "results:",
        "result_content:",
        "file_range:",
        "file:",
        "diff:",
        "process_transfer:",
        "collect_workspaces",
        "cleanup_effect_orphans:",
        "cleanup_native_orphans:",
        "cleanup_transfer_orphans:",
        "cleanup_deleted_snapshots",
    ] {
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.starts_with(expected))
                .count(),
            1,
            "missing or duplicate {expected}: {calls:#?}"
        );
    }
}

#[tokio::test]
async fn service_preserves_provider_errors() {
    let provider = Arc::new(RecordingProvider::new());
    provider.fail_file_range.store(true, Ordering::Release);
    let service = service(provider.clone()).await;
    let error = service
        .file_range(uuid::Uuid::from_u128(2), 7, "file".into(), 0, 1)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        WORKSPACE_INVALID
            .error("recording provider failure")
            .to_string()
    );
    assert_eq!(provider.calls().len(), 1);
}

#[tokio::test]
async fn storage_cleanup_loop_delegates_collection_and_orphan_cleanup() {
    let provider = Arc::new(RecordingProvider::new());
    let service = Arc::new(service(provider.clone()).await);
    let shutdown = Arc::new(tokio::sync::Notify::new());
    shutdown.notify_one();

    crate::services::run_workspace_storage_cleanup(service, shutdown).await;

    assert_eq!(
        provider.calls(),
        [
            "cleanup_transfer_orphans:None",
            "cleanup_effect_orphans:None",
            "cleanup_native_orphans:None",
            "collect_workspaces",
        ]
    );
}
