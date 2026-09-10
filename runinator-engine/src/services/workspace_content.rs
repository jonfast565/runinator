//! Native revision data-plane operations.
use super::durable_workspaces::{WorkspaceContent, WorkspaceService};
use chrono::Utc;
use runinator_models::{
    errors::{SendableError, WORKSPACE_CONFLICT, WORKSPACE_INVALID},
    workspaces::*,
};
use runinator_store::roles::DurableWorkspaceStore;

#[derive(serde::Serialize, serde::Deserialize)]
struct PageCursor {
    revision: String,
    path: String,
    results: bool,
    after: String,
}
fn decode_cursor(
    cursor: Option<String>,
    revision: &str,
    path: &str,
    results: bool,
) -> Result<Option<String>, SendableError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    if cursor.len() > 32768 {
        return Err(WORKSPACE_INVALID.error("page cursor too large"));
    }
    let cursor: PageCursor = super::workspace_cursor::decode(&cursor)?;
    if cursor.revision != revision || cursor.path != path || cursor.results != results {
        return Err(WORKSPACE_INVALID.error("page cursor belongs to another directory or revision"));
    }
    Ok(Some(cursor.after))
}
fn encode_cursor(
    after: Option<String>,
    revision: &str,
    path: &str,
    results: bool,
) -> Result<Option<String>, SendableError> {
    after
        .map(|after| {
            super::workspace_cursor::encode(&PageCursor {
                revision: revision.into(),
                path: path.into(),
                results,
                after,
            })
        })
        .transpose()
}

impl<T: DurableWorkspaceStore> WorkspaceService<T> {
    pub(super) fn objects(
        &self,
        workspace: uuid::Uuid,
    ) -> runinator_workspace::storage::cache::BufferedStore<
        super::workspace_objects::SharedObjects<T>,
    > {
        runinator_workspace::storage::cache::BufferedStore::new(
            super::workspace_objects::SharedObjects {
                db: self.store.clone(),
                blobs: self.blobs.clone(),
                workspace,
                runtime: tokio::runtime::Handle::current(),
                reader: None,
                records: runinator_workspace::storage::cache::ByteCache::new(64 * 1024 * 1024),
            },
            48 * 1024 * 1024,
        )
    }
    pub(super) async fn version_objects(
        &self,
        workspace: uuid::Uuid,
        version: i64,
    ) -> Result<
        runinator_workspace::storage::cache::BufferedStore<
            super::workspace_objects::SharedObjects<T>,
        >,
        SendableError,
    > {
        let guard =
            super::workspace_objects::ReaderGuard::new(self.store.clone(), workspace, version)
                .await?;
        let mut objects = self.objects(workspace);
        objects.inner.reader = Some(guard);
        Ok(objects)
    }
    pub async fn object(
        &self,
        workspace: uuid::Uuid,
        id: String,
        version: i64,
    ) -> Result<Vec<u8>, SendableError> {
        use runinator_workspace::storage::{record, store::ReadStore};
        let store = if version > 0 {
            self.version_objects(workspace, version).await?
        } else {
            self.objects(workspace)
        };
        tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let object = store.get(id.parse()?)?;
            let mut bytes = Vec::new();
            record::write(&mut bytes, object.kind, &object.bytes)?;
            Ok(bytes)
        })
        .await?
    }
    pub async fn upload_pack(&self, id: uuid::Uuid, bytes: Vec<u8>) -> Result<(), SendableError> {
        use runinator_workspace::storage::{self, Id};
        let checkout = self.checkout(id).await?;
        if checkout.access != WorkspaceAccess::Write || checkout.leased_until <= Utc::now() {
            return Err(WORKSPACE_CONFLICT.error("checkout is not writable"));
        }
        if bytes.len() > 80 * 1024 * 1024 {
            return Err(WORKSPACE_INVALID.error("pack exceeds 80 MiB"));
        }
        let scope = checkout.id;
        let (bytes, locations) =
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                use std::io::Write;
                let mut file = tempfile::NamedTempFile::new()?;
                file.write_all(&bytes)?;
                let digest = Id::sha256(&bytes);
                let mut locations = Vec::new();
                storage::record::index_pack(file.path(), digest, |location, info| {
                    locations.push(WorkspaceObjectLocation {
                        kind: info.kind as u8,
                        raw_len: info.raw_len as u64,
                        id: location.id.to_string(),
                        pack: format!("{scope}/{digest}"),
                        offset: location.offset,
                        length: location.length,
                        member: location.member,
                    });
                    Ok(())
                })?;
                Ok((bytes, locations))
            })
            .await??;
        crate::artifact_storage::put_workspace_pack(
            &self.blobs,
            checkout.workspace_id,
            scope,
            bytes,
        )
        .await?;
        for batch in locations.chunks(1000) {
            self.store
                .stage_workspace_objects(checkout.clone(), batch.to_vec())
                .await?;
        }
        Ok(())
    }
    pub async fn seal(
        &self,
        id: uuid::Uuid,
        request: WorkspaceSeal,
    ) -> Result<WorkspaceReceipt, SendableError> {
        use runinator_workspace::storage::{
            self,
            model::{Kind, Revision},
            store::load,
            view::View,
        };
        let checkout = self.checkout(id).await?;
        if checkout.access != WorkspaceAccess::Write {
            return Err(WORKSPACE_INVALID.error("read-only checkout cannot save"));
        }
        let remaining = (checkout.leased_until - Utc::now())
            .to_std()
            .map_err(|_| WORKSPACE_CONFLICT.error("checkout lease expired before validation"))?;
        let deadline = tokio::time::Instant::now() + remaining;
        let guard = super::workspace_validation::ValidationGuard::new();
        let objects = super::workspace_seal_objects::SealObjects::new(
            self.objects(checkout.workspace_id).inner,
        );
        let store = guard.store(objects);
        let revision_id: storage::Id = request.revision_id.parse()?;
        let parent = if checkout.base_version > 0 {
            Some(
                self.snapshot(checkout.workspace_id, checkout.base_version)
                    .await?
                    .revision_id
                    .parse::<storage::Id>()?,
            )
        } else {
            None
        };
        let limits = checkout.limits;
        let usage = tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                let scratch = tempfile::tempdir()?;
                let revision: Revision = load(&store, revision_id, Kind::Revision)?;
                if revision.parent != parent {
                    return Err(WORKSPACE_CONFLICT.error("revision has a different base"));
                }
                storage::gc::verify_roots_with_verified_info(
                    &store,
                    &[revision_id],
                    scratch.path(),
                    false,
                )?;
                let view = View::new(&store, revision_id)?;
                let usage = runinator_workspace::revision::usage(&view)?;
                limits.check(usage)?;
                // validate named values and all portable links before issuing a receipt.
                runinator_workspace::revision::validate_results(&view)?;
                runinator_workspace::revision::validate_links(&view)?;
                Ok(usage)
            }),
        )
        .await
        .map_err(|_| WORKSPACE_CONFLICT.error("checkout lease expired during validation"))???;
        let snapshot = WorkspaceSnapshot {
            workspace_id: checkout.workspace_id,
            version: checkout
                .base_version
                .checked_add(1)
                .ok_or_else(|| WORKSPACE_INVALID.error("version overflow"))?,
            parent_version: checkout.base_version,
            origin: WorkspaceOrigin::Workflow {
                workflow_run_id: checkout.workflow_run_id,
                effect_id: checkout.effect_id,
                attempt: checkout.attempt,
            },
            revision_id: request.revision_id,
            usage,
            limits,
            created_at: Utc::now(),
        };
        let receipt = WorkspaceReceipt {
            id: uuid::Uuid::now_v7(),
            checkout,
            snapshot,
        };
        self.store.save_workspace_receipt(receipt.clone()).await?;
        Ok(receipt)
    }
    pub async fn directory(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        use runinator_workspace::storage::{model::InodeData, view::View};
        if limit == 0 || limit > 1000 {
            return Err(WORKSPACE_INVALID.error("directory page size must be 1 to 1000"));
        }
        let snapshot = self.snapshot(id, version).await?;
        let after = decode_cursor(after, &snapshot.revision_id, &path, false)?;
        let store = self.version_objects(id, version).await?;
        tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let view = View::new(store, snapshot.revision_id.parse()?)?;
            let mut entries = view.directory(&path, after.as_deref(), limit + 1)?;
            let next_cursor = if entries.len() > limit {
                entries.truncate(limit);
                entries.last().map(|entry| entry.name.clone())
            } else {
                None
            };
            let entries = entries
                .into_iter()
                .map(|entry| {
                    let (kind, content_id, link_target) = match entry.inode.data {
                        InodeData::File(id) => ("file", id.to_string(), None),
                        InodeData::Directory(_) => ("directory", entry.inode_id.to_string(), None),
                        InodeData::Symlink(target) => {
                            ("symlink", entry.inode_id.to_string(), Some(target))
                        }
                    };
                    WorkspaceEntry {
                        name: entry.name,
                        kind: kind.into(),
                        inode_number: entry.inode_number,
                        content_id,
                        size_bytes: entry.size,
                        executable: entry.inode.metadata.mode & 0o111 != 0,
                        link_target,
                    }
                })
                .collect();
            let next_cursor = encode_cursor(next_cursor, &snapshot.revision_id, &path, false)?;
            Ok(WorkspaceDirectory {
                revision_id: snapshot.revision_id,
                path,
                entries,
                next_cursor,
            })
        })
        .await?
    }
    pub async fn diff(
        &self,
        id: uuid::Uuid,
        before: i64,
        after: i64,
        cursor: Option<String>,
    ) -> Result<WorkspaceDiff, SendableError> {
        let left = self.snapshot(id, before).await?;
        let right = self.snapshot(id, after).await?;
        let right_guard =
            super::workspace_objects::ReaderGuard::new(self.store.clone(), id, after).await?;
        let store = self.version_objects(id, before).await?;
        if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 1024 * 1024)
        {
            return Err(WORKSPACE_INVALID.error("diff cursor exceeds 1 MiB"));
        }
        tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let _right_guard = right_guard;
            use runinator_workspace::storage::{diff, view::View};
            let a = View::new(&store, left.revision_id.parse()?)?;
            let b = View::new(&store, right.revision_id.parse()?)?;
            let cursor = cursor
                .as_deref()
                .map(super::workspace_cursor::decode)
                .transpose()?;
            let page = diff::page(&a, &b, cursor, 200)?;
            Ok(WorkspaceDiff {
                before_revision: left.revision_id,
                after_revision: right.revision_id,
                changes: page
                    .changes
                    .into_iter()
                    .map(|change| WorkspaceDifference {
                        path: change.path,
                        result: change.result,
                        before: change.left.map(|id| id.to_string()),
                        after: change.right.map(|id| id.to_string()),
                    })
                    .collect(),
                next_cursor: page
                    .cursor
                    .map(|cursor| super::workspace_cursor::encode(&cursor))
                    .transpose()?,
            })
        })
        .await?
    }
    pub async fn results(
        &self,
        id: uuid::Uuid,
        version: i64,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        use runinator_workspace::storage::{
            model::{FileObject, Kind},
            radix,
            store::load,
            view::View,
        };
        if limit == 0 || limit > 1000 {
            return Err(WORKSPACE_INVALID.error("result page size must be 1 to 1000"));
        }
        let snapshot = self.snapshot(id, version).await?;
        let after = decode_cursor(after, &snapshot.revision_id, "", true)?;
        let store = self.version_objects(id, version).await?;
        tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let view = View::new(store, snapshot.revision_id.parse()?)?;
            let mut page = radix::page(
                &view.store,
                view.attachments,
                after.as_deref().map(str::as_bytes),
                limit + 1,
            )?;
            let next_cursor = if page.len() > limit {
                page.truncate(limit);
                page.last()
                    .map(|(key, _)| String::from_utf8(key.clone()))
                    .transpose()?
            } else {
                None
            };
            let mut entries = Vec::new();
            for (name, id) in page {
                let file: FileObject = load(&view.store, id, Kind::File)?;
                entries.push(WorkspaceEntry {
                    name: String::from_utf8(name)?,
                    kind: "result".into(),
                    inode_number: 0,
                    content_id: id.to_string(),
                    size_bytes: file.size,
                    executable: false,
                    link_target: None,
                });
            }
            let next_cursor = encode_cursor(next_cursor, &snapshot.revision_id, "", true)?;
            Ok(WorkspaceDirectory {
                revision_id: snapshot.revision_id,
                path: String::new(),
                entries,
                next_cursor,
            })
        })
        .await?
    }
    pub async fn result_content(
        &self,
        id: uuid::Uuid,
        version: i64,
        name: String,
        preview: bool,
    ) -> Result<WorkspaceContent, SendableError> {
        use runinator_workspace::storage::{
            cache::ByteCache,
            model::{FileObject, Kind},
            pages, radix,
            store::load,
            view::View,
        };
        let snapshot = self.snapshot(id, version).await?;
        let store = self.version_objects(id, version).await?;
        let (view, file, size) =
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                let view = View::new(store, snapshot.revision_id.parse()?)?;
                let id = radix::get(&view.store, view.attachments, name.as_bytes())?.ok_or_else(
                    || runinator_models::errors::WORKSPACE_MISSING.error("result not found"),
                )?;
                let size = load::<FileObject, _>(&view.store, id, Kind::File)?.size;
                Ok((view, id, if preview { size.min(1024 * 1024) } else { size }))
            })
            .await??;
        Ok(super::workspace_stream::stream(size, move |mut writer| {
            use std::io::{Read, Write};
            let cache = ByteCache::new(8 * 1024 * 1024);
            let reader = pages::FileReader::new(&view.store, &cache, file)?;
            std::io::copy(&mut reader.take(size), &mut writer)?;
            writer.flush()?;
            Ok(())
        }))
    }
    pub async fn file_range(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, SendableError> {
        let snapshot = self.snapshot(id, version).await?;
        let store = self.version_objects(id, version).await?;
        tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let view = runinator_workspace::storage::view::View::new(
                store,
                snapshot.revision_id.parse()?,
            )?;
            Ok(view.read_range(&path, offset, length)?)
        })
        .await?
    }
    pub async fn file(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
    ) -> Result<WorkspaceContent, SendableError> {
        use runinator_workspace::storage::{
            model::{FileObject, InodeData, Kind},
            store::load,
            view::View,
        };
        let snapshot = self.snapshot(id, version).await?;
        let store = self.version_objects(id, version).await?;
        let (view, size) = tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let view = View::new(store, snapshot.revision_id.parse()?)?;
            let (_, inode) = view.stat(&path)?;
            let InodeData::File(file) = inode.data else {
                return Err(WORKSPACE_INVALID.error("path is not a regular file"));
            };
            let size = load::<FileObject, _>(&view.store, file, Kind::File)?.size;
            Ok(((view, path), size))
        })
        .await??;
        Ok(super::workspace_stream::stream(size, move |writer| {
            view.0.copy_to(&view.1, writer)?;
            Ok(())
        }))
    }
    pub async fn download_ticket(
        &self,
        workspace_id: uuid::Uuid,
        version: i64,
        request: WorkspaceDownloadRequest,
    ) -> Result<WorkspaceDownload, SendableError> {
        if let Some(transfer) = request.transfer_id {
            let job = self.transfer(transfer).await?;
            if job.workspace_id != workspace_id
                || job.version != version
                || job.importing
                || job.state != "ready"
            {
                return Err(
                    WORKSPACE_INVALID.error("download transfer does not match selected version")
                );
            }
        }
        let download = WorkspaceDownload {
            transfer_id: request.transfer_id,
            id: uuid::Uuid::new_v4(),
            workspace_id,
            version,
            path: request.path,
            result: request.result,
            expires_at: Utc::now() + chrono::Duration::hours(1),
        };
        self.store
            .create_workspace_download(download.clone())
            .await?;
        Ok(download)
    }
    pub async fn ticket_content(
        &self,
        ticket: uuid::Uuid,
    ) -> Result<WorkspaceContent, SendableError> {
        let download = self
            .store
            .fetch_workspace_download(ticket)
            .await?
            .ok_or_else(|| {
                runinator_models::errors::WORKSPACE_MISSING.error("download ticket expired")
            })?;
        if let Some(id) = download.transfer_id {
            return self.transfer_content(id).await;
        }
        match download.path {
            Some(path) if download.result => {
                self.result_content(download.workspace_id, download.version, path, false)
                    .await
            }
            Some(path) => {
                self.file(download.workspace_id, download.version, path)
                    .await
            }
            None => {
                Err(WORKSPACE_INVALID
                    .error("archive download requires a completed export transfer"))
            }
        }
    }
}
