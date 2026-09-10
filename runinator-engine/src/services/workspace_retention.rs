//! Generation-fenced pack collection over explicit retained revision roots.
use super::workspace_storage::ObjectGraphStorageProvider;
use runinator_models::{
    errors::{SendableError, WORKSPACE_CONFLICT},
    workspaces::*,
};
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Renewal {
    task: tokio::task::JoinHandle<()>,
    valid: Arc<AtomicBool>,
}

struct RevocableStore<S> {
    inner: S,
    valid: Arc<AtomicBool>,
}

impl<S: ReadStore> ReadStore for RevocableStore<S> {
    fn get(&self, id: Id) -> storage::Result<Object> {
        if !self.valid.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.get(id)
    }

    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if !self.valid.load(Ordering::Acquire) {
            return Err(storage::Error::Conflict);
        }
        self.inner.info(id)
    }
}

impl Drop for Renewal {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl<T: DurableWorkspaceStore> ObjectGraphStorageProvider<T> {
    pub(super) async fn cleanup_native_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        let page =
            crate::artifact_storage::workspace_native_upload_page(&self.blobs, cursor).await?;
        for object in page.objects {
            if object.last_modified > chrono::Utc::now() - chrono::Duration::hours(24) {
                continue;
            }
            let Some(key) = object.key.strip_prefix("native/") else {
                continue;
            };
            let parts = key.split('/').collect::<Vec<_>>();
            if parts.len() != 3 {
                continue;
            }
            let (Ok(workspace), Ok(scope)) = (
                parts[0].parse::<uuid::Uuid>(),
                parts[1].parse::<uuid::Uuid>(),
            ) else {
                continue;
            };
            let Some(digest) = parts[2].strip_suffix(".pack") else {
                continue;
            };
            let pack = format!("{scope}/{digest}");
            if !self
                .store
                .workspace_pack_needed(workspace, scope, pack.clone())
                .await?
            {
                let uri = crate::artifact_storage::workspace_pack_uri(workspace, &pack)?;
                crate::artifact_storage::delete_artifact_checked(&self.blobs, &uri).await?;
            }
        }
        Ok(page.next_continuation_token)
    }
    pub(super) async fn collect_workspace(&self, id: uuid::Uuid) -> Result<(), SendableError> {
        self.collect_retired_packs(id).await?;
        let Some(lease) = self.store.claim_workspace_gc(id).await? else {
            return Ok(());
        };
        let started = std::time::Instant::now();
        tracing::info!(
            workspace_id = %id,
            roots = lease.roots.len(),
            objects = lease.object_count,
            "workspace collection started"
        );
        let valid = Arc::new(AtomicBool::new(true));
        let renew_valid = valid.clone();
        let db = self.store.clone();
        let renewing = lease.clone();
        let renewal = Renewal {
            valid,
            task: tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    if !matches!(db.renew_workspace_gc(renewing.clone()).await, Ok(true)) {
                        renew_valid.store(false, Ordering::Release);
                        return;
                    }
                }
            }),
        };
        let objects = RevocableStore {
            inner: self.objects(id),
            valid: renewal.valid.clone(),
        };
        let db = self.store.clone();
        let blobs = self.blobs.clone();
        let runtime = tokio::runtime::Handle::current();
        let staging = lease.clone();
        let valid = renewal.valid.clone();
        let repacked = tokio::task::spawn_blocking(move || -> Result<bool, SendableError> {
            let scratch = tempfile::tempdir()?;
            let roots = staging
                .roots
                .iter()
                .map(|root| root.parse::<Id>())
                .collect::<storage::Result<Vec<_>>>()?;
            let marks = storage::gc::mark_roots(&objects, &roots, scratch.path(), false)?;
            if marks.count == staging.object_count {
                return Ok(false);
            }
            drop(marks);
            storage::packs::seal_roots(
                &objects,
                &storage::staging::EmptyStore,
                &roots,
                scratch.path(),
                |pack| {
                    if !valid.load(Ordering::Acquire) {
                        return Err(storage::Error::Conflict);
                    }
                    let bytes = std::fs::read(&pack.path)?;
                    let pack_id = runtime
                        .block_on(crate::artifact_storage::put_workspace_pack(
                            &blobs,
                            id,
                            staging.token,
                            bytes,
                        ))
                        .map_err(|error| storage::Error::Io(std::io::Error::other(error)))?;
                    let mut batch = Vec::new();
                    for n in 0..pack.index.count {
                        let location = pack.index.entry(n)?;
                        use storage::store::ReadStore;
                        let info = objects.info(location.id)?;
                        batch.push(WorkspaceObjectLocation {
                            kind: info.kind as u8,
                            raw_len: info.raw_len as u64,
                            id: location.id.to_string(),
                            pack: pack_id.clone(),
                            offset: location.offset,
                            length: location.length,
                            member: location.member,
                        });
                        if batch.len() == 1000 {
                            runtime
                                .block_on(db.stage_workspace_gc(
                                    staging.clone(),
                                    std::mem::take(&mut batch),
                                ))
                                .map_err(|error| {
                                    storage::Error::Io(std::io::Error::other(error))
                                })?;
                        }
                    }
                    if !batch.is_empty() {
                        runtime
                            .block_on(db.stage_workspace_gc(staging.clone(), batch))
                            .map_err(|error| storage::Error::Io(std::io::Error::other(error)))?;
                    }
                    Ok(())
                },
            )?;
            Ok(true)
        })
        .await??;
        if !renewal.valid.load(Ordering::Acquire) {
            return Err(WORKSPACE_CONFLICT.error("collection lease was lost"));
        }
        self.store.finish_workspace_gc(lease, repacked).await?;
        drop(renewal);
        tracing::info!(
            workspace_id = %id,
            repacked,
            duration_ms = started.elapsed().as_millis(),
            "workspace collection finished"
        );
        self.collect_retired_packs(id).await
    }

    async fn collect_retired_packs(&self, id: uuid::Uuid) -> Result<(), SendableError> {
        for pack in self.store.expired_workspace_packs(id).await? {
            let uri = crate::artifact_storage::workspace_pack_uri(id, &pack)?;
            crate::artifact_storage::delete_artifact_checked(&self.blobs, &uri).await?;
            self.store.finish_workspace_pack_cleanup(id, pack).await?;
        }
        Ok(())
    }
}
