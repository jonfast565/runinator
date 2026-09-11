//! resumable migration from the legacy data/meta trees to self-describing v2 files.

use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::errors::BlobError;
use crate::key::ObjectKey;
use crate::meta::ObjectMeta;

use super::format::{self, PartMeta, OBJECT_MAGIC, PART_MAGIC};
use super::paths::{self, BucketPaths, V2_MARKER};

pub(super) async fn migrate_root(root: &Path, buffer_size: usize) -> Result<(), BlobError> {
    let mut entries = fs::read_dir(root)
        .await
        .map_err(|err| BlobError::Io(format!("listing blob root {}: {err}", root.display())))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| BlobError::Io(format!("listing blob root {}: {err}", root.display())))?
    {
        if !entry
            .file_type()
            .await
            .map_err(|err| BlobError::Io(format!("stating {}: {err}", entry.path().display())))?
            .is_dir()
        {
            continue;
        }
        let Some(bucket) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let paths = BucketPaths::new(root, &bucket);
        if !fs::try_exists(paths.marker()).await.unwrap_or(false) {
            continue;
        }
        let marker = fs::read(paths.marker()).await.unwrap_or_default();
        if marker == V2_MARKER {
            continue;
        }
        migrate_bucket(&bucket, &paths, buffer_size).await?;
    }
    Ok(())
}

async fn migrate_bucket(
    bucket: &str,
    paths: &BucketPaths,
    buffer_size: usize,
) -> Result<(), BlobError> {
    let started = std::time::Instant::now();
    let _ = fs::remove_dir_all(paths.tmp_root()).await;
    for dir in [paths.objects_root(), paths.uploads_root(), paths.tmp_root()] {
        fs::create_dir_all(&dir)
            .await
            .map_err(|err| BlobError::Io(format!("creating {}: {err}", dir.display())))?;
    }

    let blocking_paths = paths.clone();
    let keys =
        tokio::task::spawn_blocking(move || super::walk::collect_legacy_keys(&blocking_paths))
            .await
            .map_err(|err| BlobError::Io(format!("collecting legacy blob keys: {err}")))??;
    let mut bytes = 0u64;
    for (index, raw_key) in keys.iter().enumerate() {
        let key = ObjectKey::parse(raw_key)?;
        let encoded = fs::read(paths.legacy_meta(&key))
            .await
            .map_err(|err| BlobError::Io(format!("reading legacy metadata for {key}: {err}")))?;
        let meta: ObjectMeta = serde_json::from_slice(&encoded)
            .map_err(|err| BlobError::Io(format!("parsing legacy metadata for {key}: {err}")))?;
        if meta.key != raw_key.as_str() {
            return Err(BlobError::Io(format!(
                "legacy metadata key '{}' disagrees with path '{raw_key}'",
                meta.key
            )));
        }
        let final_path = paths.object(&key);
        if fs::try_exists(&final_path).await.unwrap_or(false) {
            let mut file = fs::File::open(&final_path)
                .await
                .map_err(|err| BlobError::Io(format!("opening {}: {err}", final_path.display())))?;
            let (existing, _) = format::read_object(&mut file, &final_path).await?;
            if existing.key != meta.key
                || existing.size != meta.size
                || !existing.sha256.eq_ignore_ascii_case(&meta.sha256)
            {
                return Err(BlobError::Io(format!(
                    "migrated object {} disagrees with its legacy metadata",
                    final_path.display()
                )));
            }
            verify_payload(&mut file, &final_path, &existing, buffer_size).await?;
        } else {
            migrate_object(paths, &key, &meta, buffer_size).await?;
        }
        bytes = bytes.saturating_add(meta.size);
        fs::remove_file(paths.legacy_data(&key))
            .await
            .map_err(|err| BlobError::Io(format!("removing legacy data for {key}: {err}")))?;
        fs::remove_file(paths.legacy_meta(&key))
            .await
            .map_err(|err| BlobError::Io(format!("removing legacy metadata for {key}: {err}")))?;
        if (index + 1) % 1000 == 0 {
            tracing::info!(
                bucket,
                objects = index + 1,
                bytes,
                "blob layout migration progress"
            );
        }
    }

    migrate_upload_parts(paths, buffer_size).await?;
    for legacy in [paths.legacy_data_root(), paths.legacy_meta_root()] {
        if fs::try_exists(&legacy).await.unwrap_or(false) {
            fs::remove_dir_all(&legacy)
                .await
                .map_err(|err| BlobError::Io(format!("removing {}: {err}", legacy.display())))?;
        }
    }
    let (marker_tmp, mut marker_file) = paths::create_staged(paths, "bucket-marker").await?;
    marker_file
        .write_all(V2_MARKER)
        .await
        .map_err(|err| BlobError::Io(format!("writing v2 bucket marker: {err}")))?;
    marker_file
        .sync_all()
        .await
        .map_err(|err| BlobError::Io(format!("syncing v2 bucket marker: {err}")))?;
    drop(marker_file);
    paths::commit_replace(&marker_tmp, &paths.marker()).await?;
    tracing::info!(
        bucket,
        objects = keys.len(),
        bytes,
        elapsed_ms = started.elapsed().as_millis(),
        "blob layout migration completed"
    );
    Ok(())
}

async fn verify_payload(
    file: &mut fs::File,
    path: &Path,
    meta: &ObjectMeta,
    buffer_size: usize,
) -> Result<(), BlobError> {
    use tokio::io::AsyncSeekExt;

    file.seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
    let mut remaining = meta.size;
    let mut buffer = vec![0u8; buffer_size];
    let mut hasher = Sha256::new();
    while remaining != 0 {
        let wanted = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = file
            .read(&mut buffer[..wanted])
            .await
            .map_err(|err| BlobError::Io(format!("reading {}: {err}", path.display())))?;
        if read == 0 {
            return Err(BlobError::Io(format!(
                "migrated object {} is truncated",
                path.display()
            )));
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    let actual = hex::encode(hasher.finalize());
    if actual.eq_ignore_ascii_case(&meta.sha256) {
        return Ok(());
    }
    Err(BlobError::DigestMismatch {
        expected: meta.sha256.clone(),
        actual,
    })
}

async fn migrate_object(
    paths: &BucketPaths,
    key: &ObjectKey,
    meta: &ObjectMeta,
    buffer_size: usize,
) -> Result<(), BlobError> {
    let source_path = paths.legacy_data(key);
    let mut source = fs::File::open(&source_path)
        .await
        .map_err(|err| paths::read_error(&source_path, key.as_str(), err))?;
    let (staged, mut target) =
        paths::create_staged(paths, &format!("migrate-{}", uuid::Uuid::now_v7())).await?;
    let result = async {
        let (size, sha256) = copy_hashed(&mut source, &mut target, buffer_size).await?;
        if size != meta.size || !sha256.eq_ignore_ascii_case(&meta.sha256) {
            return Err(BlobError::DigestMismatch {
                expected: format!("{} bytes / {}", meta.size, meta.sha256),
                actual: format!("{size} bytes / {sha256}"),
            });
        }
        format::append_footer(&mut target, size, meta, OBJECT_MAGIC).await?;
        target
            .sync_all()
            .await
            .map_err(|err| BlobError::Io(format!("syncing migrated object: {err}")))?;
        drop(target);
        paths::commit_replace(&staged, &paths.object(key)).await
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&staged).await;
    }
    result
}

async fn migrate_upload_parts(paths: &BucketPaths, buffer_size: usize) -> Result<(), BlobError> {
    let mut uploads = match fs::read_dir(paths.uploads_root()).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(BlobError::Io(format!("listing multipart uploads: {err}"))),
    };
    while let Some(upload) = uploads
        .next_entry()
        .await
        .map_err(|err| BlobError::Io(format!("listing multipart uploads: {err}")))?
    {
        if !upload
            .file_type()
            .await
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let mut parts = fs::read_dir(upload.path())
            .await
            .map_err(|err| BlobError::Io(format!("listing {}: {err}", upload.path().display())))?;
        while let Some(part) = parts
            .next_entry()
            .await
            .map_err(|err| BlobError::Io(format!("listing {}: {err}", upload.path().display())))?
        {
            let name = part.file_name().to_string_lossy().into_owned();
            if !name.starts_with("part-")
                || !part
                    .file_type()
                    .await
                    .map(|kind| kind.is_file())
                    .unwrap_or(false)
            {
                continue;
            }
            let path = part.path();
            let already_v2 = if let Ok(mut file) = fs::File::open(&path).await {
                format::read_part(&mut file, &path).await.is_ok()
            } else {
                false
            };
            if already_v2 {
                continue;
            }
            let mut source = fs::File::open(&path)
                .await
                .map_err(|err| BlobError::Io(format!("opening {}: {err}", path.display())))?;
            let (staged, mut target) =
                paths::create_staged(paths, &format!("part-migrate-{}", uuid::Uuid::now_v7()))
                    .await?;
            let result = async {
                let (size, sha256) = copy_hashed(&mut source, &mut target, buffer_size).await?;
                format::append_footer(&mut target, size, &PartMeta { size, sha256 }, PART_MAGIC)
                    .await?;
                target
                    .sync_all()
                    .await
                    .map_err(|err| BlobError::Io(format!("syncing migrated part: {err}")))?;
                drop(target);
                paths::commit_replace(&staged, &path).await
            }
            .await;
            if result.is_err() {
                let _ = fs::remove_file(&staged).await;
            }
            result?;
        }
    }
    Ok(())
}

pub(super) async fn copy_hashed(
    reader: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
    writer: &mut tokio::fs::File,
    buffer_size: usize,
) -> Result<(u64, String), BlobError> {
    let mut buffer = vec![0u8; buffer_size];
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    loop {
        let read = reader.read(&mut buffer).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::InvalidData {
                BlobError::BadRequest(err.to_string())
            } else {
                BlobError::Io(format!("reading blob stream: {err}"))
            }
        })?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|err| BlobError::Io(format!("writing blob stream: {err}")))?;
        hasher.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| BlobError::BadRequest("blob size overflow".into()))?;
    }
    Ok((size, hex::encode(hasher.finalize())))
}
