//! Restartable transfers with shared archives, fenced publication, and cancellation.
use super::durable_workspaces::{WorkspaceContent, WorkspaceService};
use runinator_models::{
    errors::{SendableError, WORKSPACE_CONFLICT, WORKSPACE_INVALID, WORKSPACE_MISSING},
    workspaces::*,
};
use runinator_store::roles::DurableWorkspaceStore;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use uuid::Uuid;

struct Progress {
    alive: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Progress {
    fn drop(&mut self) {
        self.task.abort();
        self.alive.store(false, Ordering::Release);
    }
}
impl Progress {
    fn start<T: DurableWorkspaceStore>(store: Arc<T>, job: WorkspaceTransfer) -> Self {
        let alive = Arc::new(AtomicBool::new(true));
        let bytes = Arc::new(AtomicU64::new(0));
        let valid = alive.clone();
        let count = bytes.clone();
        let task = tokio::spawn(async move {
            loop {
                if !matches!(
                    store
                        .progress_workspace_transfer(job.clone(), count.load(Ordering::Acquire))
                        .await,
                    Ok(true)
                ) {
                    valid.store(false, Ordering::Release);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        Self { alive, bytes, task }
    }
}
struct GuardedStore<S> {
    inner: S,
    alive: Arc<AtomicBool>,
}
impl<S: runinator_workspace::storage::store::ReadStore>
    runinator_workspace::storage::store::ReadStore for GuardedStore<S>
{
    fn get(
        &self,
        id: runinator_workspace::storage::Id,
    ) -> runinator_workspace::storage::Result<runinator_workspace::storage::store::Object> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(runinator_workspace::storage::Error::Conflict);
        }
        self.inner.get(id)
    }
    fn info(
        &self,
        id: runinator_workspace::storage::Id,
    ) -> runinator_workspace::storage::Result<runinator_workspace::storage::store::ObjectInfo> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(runinator_workspace::storage::Error::Conflict);
        }
        self.inner.info(id)
    }
}
struct Reader<R> {
    inner: R,
    alive: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
}
impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Reader<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.alive.load(Ordering::Acquire) {
            return std::task::Poll::Ready(Err(std::io::Error::other(
                "transfer cancelled or lease lost",
            )));
        }
        let before = buf.filled().len();
        let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        self.bytes
            .fetch_add((buf.filled().len() - before) as u64, Ordering::Release);
        result
    }
}
fn budget(limits: WorkspaceLimits) -> Result<u64, SendableError> {
    limits
        .max_bytes
        .checked_mul(3)
        .and_then(|n| {
            limits
                .max_entries
                .checked_mul(8192)
                .and_then(|m| n.checked_add(m))
        })
        .and_then(|n| n.checked_add(128 * 1024 * 1024))
        .ok_or_else(|| WORKSPACE_INVALID.error("transfer budget overflow"))
}
impl<T: DurableWorkspaceStore> WorkspaceService<T> {
    pub async fn create_transfer(
        &self,
        workspace_id: Uuid,
        version: i64,
        importing: bool,
        filesystem: bool,
    ) -> Result<WorkspaceTransfer, SendableError> {
        let now = chrono::Utc::now();
        let limits = if importing {
            self.limits
        } else {
            self.snapshot(workspace_id, version).await?.limits
        };
        let job = WorkspaceTransfer {
            filesystem,
            id: Uuid::now_v7(),
            workspace_id,
            version,
            importing,
            state: if importing { "uploading" } else { "queued" }.into(),
            bytes_processed: 0,
            error: None,
            limits,
            created_at: now,
            expires_at: now + chrono::Duration::days(7),
            token: Uuid::nil(),
            archive_uri: None,
        };
        budget(limits)?;
        self.store.create_workspace_transfer(job.clone()).await?;
        Ok(job)
    }
    pub async fn transfer(&self, id: Uuid) -> Result<WorkspaceTransfer, SendableError> {
        self.store
            .fetch_workspace_transfer(id)
            .await?
            .ok_or_else(|| WORKSPACE_MISSING.error("transfer not found"))
    }
    pub async fn cancel_transfer(&self, id: Uuid) -> Result<bool, SendableError> {
        self.store.cancel_workspace_transfer(id).await
    }
    pub async fn upload_transfer<R: tokio::io::AsyncRead + Send + Unpin>(
        &self,
        id: Uuid,
        reader: R,
    ) -> Result<WorkspaceTransfer, SendableError> {
        let job = self
            .store
            .claim_workspace_transfer(id, true)
            .await?
            .ok_or_else(|| WORKSPACE_CONFLICT.error("transfer is not awaiting upload"))?;
        let progress = Progress::start(self.store.clone(), job.clone());
        let reader = Reader {
            inner: reader,
            alive: progress.alive.clone(),
            bytes: progress.bytes.clone(),
        };
        let result = crate::artifact_storage::put_workspace_transfer(
            &self.blobs,
            job.id,
            job.token,
            reader,
            budget(job.limits)?,
            Some(std::time::Duration::from_secs(60)),
        )
        .await;
        self.store
            .progress_workspace_transfer(job.clone(), progress.bytes.load(Ordering::Acquire))
            .await?;
        match result {
            Ok(uri) => {
                self.store
                    .finish_workspace_transfer(job.clone(), "queued".into(), Some(uri), None, None)
                    .await?
            }
            Err(error) => {
                let _ = self
                    .store
                    .finish_workspace_transfer(
                        job,
                        "failed".into(),
                        None,
                        None,
                        Some(error.to_string()),
                    )
                    .await;
                return Err(error);
            }
        }
        self.transfer(id).await
    }
    pub async fn transfer_content(&self, id: Uuid) -> Result<WorkspaceContent, SendableError> {
        let job = self.transfer(id).await?;
        if job.importing || job.state != "ready" || job.expires_at <= chrono::Utc::now() {
            return Err(WORKSPACE_CONFLICT.error("export is not ready"));
        }
        crate::artifact_storage::open_artifact(
            &self.blobs,
            job.archive_uri
                .as_deref()
                .ok_or_else(|| WORKSPACE_MISSING.error("export archive missing"))?,
            None,
        )
        .await
    }
    pub(super) async fn cleanup_transfer_archives(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        let page =
            crate::artifact_storage::workspace_transfer_upload_page(&self.blobs, cursor).await?;
        for object in page.objects {
            if object.last_modified > chrono::Utc::now() - chrono::Duration::hours(24) {
                continue;
            }
            let Some(path) = object.key.strip_prefix("transfers/") else {
                continue;
            };
            let Some((id, _)) = path.split_once('/') else {
                continue;
            };
            let Ok(id) = id.parse::<Uuid>() else {
                continue;
            };
            let key = runinator_blob_core::ObjectKey::parse(&object.key)?;
            let uri = runinator_blob_core::blob_uri(runinator_blob_core::WORKSPACE_BUCKET, &key);
            if let Some(job) = self.store.fetch_workspace_transfer(id).await?
                && job.expires_at > chrono::Utc::now()
                && (job.archive_uri.as_ref() == Some(&uri)
                    || ["receiving", "running"].contains(&job.state.as_str()))
            {
                continue;
            }
            crate::artifact_storage::delete_artifact_checked(&self.blobs, &uri).await?;
        }
        Ok(page.next_continuation_token)
    }
    pub async fn run_transfers(&self) -> Result<(), SendableError> {
        for id in self.store.workspace_transfer_candidates().await? {
            let Some(job) = self.store.claim_workspace_transfer(id, false).await? else {
                continue;
            };
            let progress = Progress::start(self.store.clone(), job.clone());
            let result = if job.importing {
                self.import_transfer(&job, &progress).await
            } else {
                self.export_transfer(&job, &progress).await
            };
            if let Err(error) = result {
                tracing::warn!(%error, transfer_id = %id, "workspace transfer failed");
                let _ = self
                    .store
                    .finish_workspace_transfer(
                        job,
                        "failed".into(),
                        None,
                        None,
                        Some(error.to_string()),
                    )
                    .await;
            }
        }
        Ok(())
    }
    async fn export_transfer(
        &self,
        job: &WorkspaceTransfer,
        progress: &Progress,
    ) -> Result<(), SendableError> {
        let snapshot = self.snapshot(job.workspace_id, job.version).await?;
        let objects = GuardedStore {
            inner: self.version_objects(job.workspace_id, job.version).await?,
            alive: progress.alive.clone(),
        };
        let (writer, reader) = tokio::io::duplex(512 * 1024);
        let runtime = tokio::runtime::Handle::current();
        let filesystem = job.filesystem;
        let producer = tokio::task::spawn_blocking(move || -> Result<(), SendableError> {
            struct Writer(tokio::io::DuplexStream, tokio::runtime::Handle);
            impl std::io::Write for Writer {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    use tokio::io::AsyncWriteExt;
                    self.1.block_on(self.0.write(bytes))
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    use tokio::io::AsyncWriteExt;
                    self.1.block_on(self.0.flush())
                }
            }
            let scratch = tempfile::tempdir()?;
            if filesystem {
                runinator_workspace::filesystem::export(
                    &objects,
                    snapshot.revision_id.parse()?,
                    scratch.path(),
                    Writer(writer, runtime),
                )
            } else {
                runinator_workspace::native::export(
                    &objects,
                    snapshot.revision_id.parse()?,
                    scratch.path(),
                    Writer(writer, runtime),
                )
            }
        });
        let reader = Reader {
            inner: reader,
            alive: progress.alive.clone(),
            bytes: progress.bytes.clone(),
        };
        let stored = crate::artifact_storage::put_workspace_transfer(
            &self.blobs,
            job.id,
            job.token,
            reader,
            budget(job.limits)?,
            None,
        )
        .await;
        // never publish a truncated archive if the producer failed after emitting bytes.
        let produced = producer.await?;
        let uri = stored?;
        produced?;
        self.store
            .progress_workspace_transfer(job.clone(), progress.bytes.load(Ordering::Acquire))
            .await?;
        self.store
            .finish_workspace_transfer(job.clone(), "ready".into(), Some(uri), None, None)
            .await
    }
    async fn import_transfer(
        &self,
        job: &WorkspaceTransfer,
        progress: &Progress,
    ) -> Result<(), SendableError> {
        use tokio::io::AsyncWriteExt;
        let archive = crate::artifact_storage::open_artifact(
            &self.blobs,
            job.archive_uri
                .as_deref()
                .ok_or_else(|| WORKSPACE_MISSING.error("import archive missing"))?,
            None,
        )
        .await?;
        if archive.size_bytes > budget(job.limits)? {
            return Err(WORKSPACE_INVALID.error("archive exceeds transfer budget"));
        }
        let mut input = Reader {
            inner: archive.body,
            alive: progress.alive.clone(),
            bytes: progress.bytes.clone(),
        };
        let file = tempfile::tempfile()?;
        let mut file = tokio::fs::File::from_std(file);
        tokio::io::copy(&mut input, &mut file).await?;
        file.flush().await?;
        let file = file.into_std().await;
        let db = self.store.clone();
        let blobs = self.blobs.clone();
        let runtime = tokio::runtime::Handle::current();
        let owned = job.clone();
        let valid = progress.alive.clone();
        let snapshot = tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            use runinator_workspace::storage::{self, store::ReadStore};
            use std::io::{Seek, SeekFrom};
            let scratch = tempfile::tempdir()?;
            let mut file = file;
            file.seek(SeekFrom::Start(0))?;
            let (objects, revision, usage, format) =
                runinator_workspace::native::import_with_format(
                    file,
                    scratch.path(),
                    owned.limits,
                )?;
            storage::packs::seal(
                &objects,
                &storage::staging::EmptyStore,
                revision,
                scratch.path(),
                |pack| {
                    if !valid.load(Ordering::Acquire) {
                        return Err(storage::Error::Conflict);
                    }
                    let key = runtime
                        .block_on(crate::artifact_storage::put_workspace_pack(
                            &blobs,
                            owned.workspace_id,
                            owned.token,
                            std::fs::read(pack.path)?,
                        ))
                        .map_err(|error| storage::Error::Io(std::io::Error::other(error)))?;
                    let mut batch = Vec::new();
                    for n in 0..pack.index.count {
                        let location = pack.index.entry(n)?;
                        let info = objects.info(location.id)?;
                        batch.push(WorkspaceObjectLocation {
                            id: location.id.to_string(),
                            kind: info.kind as u8,
                            raw_len: info.raw_len as u64,
                            pack: key.clone(),
                            offset: location.offset,
                            length: location.length,
                            member: location.member,
                        });
                        if batch.len() == 1000 {
                            runtime
                                .block_on(db.stage_workspace_transfer(
                                    owned.clone(),
                                    std::mem::take(&mut batch),
                                ))
                                .map_err(|error| {
                                    storage::Error::Io(std::io::Error::other(error))
                                })?;
                        }
                    }
                    if !batch.is_empty() {
                        runtime
                            .block_on(db.stage_workspace_transfer(owned.clone(), batch))
                            .map_err(|error| storage::Error::Io(std::io::Error::other(error)))?;
                    }
                    Ok(())
                },
            )?;
            Ok(WorkspaceSnapshot {
                workspace_id: owned.workspace_id,
                version: 1,
                parent_version: 0,
                origin: WorkspaceOrigin::Import {
                    transfer_id: owned.id,
                    format: format.into(),
                },
                revision_id: revision.to_string(),
                usage,
                limits: owned.limits,
                created_at: chrono::Utc::now(),
            })
        })
        .await??;
        self.store
            .finish_workspace_transfer(
                job.clone(),
                "ready".into(),
                job.archive_uri.clone(),
                Some(snapshot),
                None,
            )
            .await
    }
}

/// Transfer workers may restart on any replica; database claims serialize each job.
pub async fn run_workspace_transfers<T: DurableWorkspaceStore>(
    service: Arc<WorkspaceService<T>>,
    shutdown: Arc<tokio::sync::Notify>,
) {
    loop {
        if let Err(error) = service.run_transfers().await {
            tracing::warn!(%error, "workspace transfer queue will retry");
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {} }
    }
}
