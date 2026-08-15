//! the local filesystem backend.
//!
//! this is what the blob service runs on top of (its container filesystem) and what a single-node or
//! desktop deployment uses directly, with no service in between. see [`paths`] for the on-disk
//! layout and the two divergences from s3 that mirroring keys onto a filesystem forces.

mod paths;
mod walk;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::errors::BlobError;
use crate::key::{validate_bucket, ObjectKey};
use crate::listing::{BucketSummary, ListRequest, ListResponse};
use crate::meta::{sha256_hex, ObjectMeta, PutOptions, DEFAULT_CONTENT_TYPE};
use crate::multipart::{CompletedPart, MAX_PART_NUMBER, MIN_PART_NUMBER};
use crate::range::ByteRange;
use crate::store::{BlobStore, ObjectReader, Result};

use paths::BucketPaths;

/// an object store backed by a directory tree.
pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    /// open (creating if needed) a store rooted at `root`.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .await
            .map_err(|err| BlobError::Io(format!("creating {}: {err}", root.display())))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// resolve a bucket, refusing one that was never created. every operation goes through here, so
    /// an unknown bucket fails the same way regardless of which call found it.
    async fn bucket(&self, bucket: &str) -> Result<BucketPaths> {
        validate_bucket(bucket)?;
        let paths = BucketPaths::new(&self.root, bucket);
        if !fs::try_exists(paths.marker()).await.unwrap_or(false) {
            return Err(BlobError::NoSuchBucket(bucket.to_string()));
        }
        Ok(paths)
    }

    async fn write_meta(&self, paths: &BucketPaths, meta: &ObjectMeta) -> Result<()> {
        let key = ObjectKey::parse(&meta.key)?;
        let encoded = serde_json::to_vec(meta)
            .map_err(|err| BlobError::Io(format!("encoding metadata for {}: {err}", meta.key)))?;
        let staged = paths::stage(paths, &staging_name("meta"), &encoded).await?;
        paths::commit_replace(&staged, &paths.meta(&key)).await
    }

    async fn read_meta(&self, paths: &BucketPaths, key: &ObjectKey) -> Result<ObjectMeta> {
        let path = paths.meta(key);
        let bytes = fs::read(&path)
            .await
            .map_err(|err| paths::read_error(&path, key.as_str(), err))?;
        serde_json::from_slice(&bytes)
            .map_err(|err| BlobError::Io(format!("parsing {}: {err}", path.display())))
    }

    /// commit already-verified bytes plus their descriptor. shared by `put` and multipart
    /// completion, which differ only in how they assembled the bytes.
    async fn commit_object(
        &self,
        paths: &BucketPaths,
        key: &ObjectKey,
        body: &[u8],
        options: &PutOptions,
        sha256: String,
    ) -> Result<ObjectMeta> {
        let staged = paths::stage(paths, &staging_name("data"), body).await?;
        let data_path = paths.data(key);
        if options.if_none_match {
            paths::commit_exclusive(&staged, &data_path)
                .await
                .map_err(|err| match err {
                    BlobError::AlreadyExists(_) => BlobError::AlreadyExists(key.to_string()),
                    other => other,
                })?;
        } else {
            paths::commit_replace(&staged, &data_path).await?;
        }
        let meta = ObjectMeta {
            key: key.as_str().to_string(),
            size: body.len() as u64,
            sha256,
            content_type: options
                .content_type
                .clone()
                .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
            last_modified: Utc::now(),
            metadata: options.metadata.clone(),
        };
        self.write_meta(paths, &meta).await?;
        Ok(meta)
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    fn backend(&self) -> &'static str {
        "fs"
    }

    async fn create_bucket(&self, bucket: &str) -> Result<()> {
        validate_bucket(bucket)?;
        let paths = BucketPaths::new(&self.root, bucket);
        for dir in [paths.data_root(), paths.meta_root(), paths.tmp_root()] {
            fs::create_dir_all(&dir)
                .await
                .map_err(|err| BlobError::Io(format!("creating {}: {err}", dir.display())))?;
        }
        fs::write(paths.marker(), b"")
            .await
            .map_err(|err| BlobError::Io(format!("creating bucket {bucket}: {err}")))
    }

    async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        if !walk::collect_keys(&paths)?.is_empty() {
            return Err(BlobError::BucketNotEmpty(bucket.to_string()));
        }
        fs::remove_dir_all(&paths.root)
            .await
            .map_err(|err| BlobError::Io(format!("removing bucket {bucket}: {err}")))
    }

    async fn bucket_exists(&self, bucket: &str) -> Result<bool> {
        validate_bucket(bucket)?;
        let paths = BucketPaths::new(&self.root, bucket);
        Ok(fs::try_exists(paths.marker()).await.unwrap_or(false))
    }

    async fn list_buckets(&self) -> Result<Vec<BucketSummary>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut buckets = Vec::new();
            let Ok(entries) = std::fs::read_dir(&root) else {
                return Ok(buckets);
            };
            for entry in entries.flatten() {
                // the marker is what distinguishes a bucket from any other directory that happens
                // to sit in the data root.
                let marker = entry.path().join(paths::BUCKET_MARKER);
                if !marker.exists() {
                    continue;
                }
                let created = std::fs::metadata(&marker)
                    .and_then(|meta| meta.created())
                    .map(chrono::DateTime::<Utc>::from)
                    .unwrap_or_else(|_| Utc::now());
                if let Some(name) = entry.file_name().to_str() {
                    buckets.push(BucketSummary {
                        name: name.to_string(),
                        created_at: created,
                    });
                }
            }
            buckets.sort_by(|left, right| left.name.cmp(&right.name));
            Ok(buckets)
        })
        .await
        .map_err(|err| BlobError::Io(format!("listing buckets failed: {err}")))?
    }

    async fn put(
        &self,
        bucket: &str,
        key: &ObjectKey,
        body: Vec<u8>,
        options: PutOptions,
    ) -> Result<ObjectMeta> {
        let paths = self.bucket(bucket).await?;
        let sha256 = sha256_hex(&body);
        // verify before anything is written, so a mismatched upload leaves no trace at all.
        if let Some(expected) = &options.expected_sha256 {
            if !expected.eq_ignore_ascii_case(&sha256) {
                return Err(BlobError::DigestMismatch {
                    expected: expected.clone(),
                    actual: sha256,
                });
            }
        }
        self.commit_object(&paths, key, &body, &options, sha256)
            .await
    }

    async fn head(&self, bucket: &str, key: &ObjectKey) -> Result<ObjectMeta> {
        let paths = self.bucket(bucket).await?;
        self.read_meta(&paths, key).await
    }

    async fn open(
        &self,
        bucket: &str,
        key: &ObjectKey,
        range: Option<ByteRange>,
    ) -> Result<ObjectReader> {
        let paths = self.bucket(bucket).await?;
        let meta = self.read_meta(&paths, key).await?;
        let path = paths.data(key);
        let mut file = fs::File::open(&path)
            .await
            .map_err(|err| paths::read_error(&path, key.as_str(), err))?;
        let Some(range) = range else {
            return Ok(ObjectReader {
                meta,
                range: None,
                body: Box::new(file),
            });
        };
        let resolved = range.resolve(meta.size)?;
        file.seek(std::io::SeekFrom::Start(resolved.start))
            .await
            .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
        Ok(ObjectReader {
            meta,
            range: Some(resolved),
            body: Box::new(file.take(resolved.length)),
        })
    }

    async fn delete(&self, bucket: &str, key: &ObjectKey) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        // s3 deletes are idempotent, so a missing object is a success rather than a 404.
        let _ = fs::remove_file(paths.data(key)).await;
        let _ = fs::remove_file(paths.meta(key)).await;
        Ok(())
    }

    async fn list(&self, bucket: &str, request: &ListRequest) -> Result<ListResponse> {
        let paths = self.bucket(bucket).await?;
        let request = request.clone();
        // the walk is blocking directory io over a potentially large tree; keep it off the reactor.
        tokio::task::spawn_blocking(move || {
            let keys = walk::collect_keys(&paths)?;
            walk::page(&paths, &keys, &request)
        })
        .await
        .map_err(|err| BlobError::Io(format!("listing task failed: {err}")))?
    }

    async fn create_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        options: PutOptions,
    ) -> Result<String> {
        let paths = self.bucket(bucket).await?;
        let upload_id = staging_name("upload");
        let dir = paths.upload(&upload_id);
        fs::create_dir_all(&dir)
            .await
            .map_err(|err| BlobError::Io(format!("creating {}: {err}", dir.display())))?;
        let manifest = UploadManifest {
            key: key.as_str().to_string(),
            content_type: options.content_type,
            metadata: options.metadata,
            if_none_match: options.if_none_match,
            expected_sha256: options.expected_sha256,
        };
        let encoded = serde_json::to_vec(&manifest)
            .map_err(|err| BlobError::Io(format!("encoding upload manifest: {err}")))?;
        fs::write(dir.join("upload.json"), encoded)
            .await
            .map_err(|err| BlobError::Io(format!("writing upload manifest: {err}")))?;
        Ok(upload_id)
    }

    async fn upload_part(
        &self,
        bucket: &str,
        _key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        body: Vec<u8>,
    ) -> Result<String> {
        if !(MIN_PART_NUMBER..=MAX_PART_NUMBER).contains(&part_number) {
            return Err(BlobError::BadRequest(format!(
                "part number {part_number} outside {MIN_PART_NUMBER}..={MAX_PART_NUMBER}"
            )));
        }
        let paths = self.bucket(bucket).await?;
        let dir = paths.upload(upload_id);
        if !fs::try_exists(dir.join("upload.json"))
            .await
            .unwrap_or(false)
        {
            return Err(BlobError::NoSuchUpload(upload_id.to_string()));
        }
        let etag = format!("\"{}\"", sha256_hex(&body));
        fs::write(dir.join(part_filename(part_number)), &body)
            .await
            .map_err(|err| BlobError::Io(format!("writing part {part_number}: {err}")))?;
        Ok(etag)
    }

    async fn complete_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<ObjectMeta> {
        let paths = self.bucket(bucket).await?;
        let dir = paths.upload(upload_id);
        let manifest = fs::read(dir.join("upload.json"))
            .await
            .map_err(|_| BlobError::NoSuchUpload(upload_id.to_string()))?;
        let manifest: UploadManifest = serde_json::from_slice(&manifest)
            .map_err(|err| BlobError::Io(format!("parsing upload manifest: {err}")))?;
        if manifest.key != key.as_str() {
            return Err(BlobError::BadRequest(format!(
                "upload {upload_id} was opened for key '{}', not '{key}'",
                manifest.key
            )));
        }

        // parts are concatenated in the order the completion request lists them, not the order they
        // arrived; s3 requires ascending part numbers, so reject anything else rather than assemble
        // an object the client did not describe.
        let mut body = Vec::new();
        let mut previous = 0;
        for part in parts {
            if part.part_number <= previous {
                return Err(BlobError::BadRequest(
                    "completion parts must be in ascending part-number order".into(),
                ));
            }
            previous = part.part_number;
            let path = dir.join(part_filename(part.part_number));
            let bytes = fs::read(&path).await.map_err(|_| {
                BlobError::BadRequest(format!("part {} was never uploaded", part.part_number))
            })?;
            let actual = format!("\"{}\"", sha256_hex(&bytes));
            if actual != part.etag {
                return Err(BlobError::DigestMismatch {
                    expected: part.etag.clone(),
                    actual,
                });
            }
            body.extend_from_slice(&bytes);
        }

        let sha256 = sha256_hex(&body);
        if let Some(expected) = &manifest.expected_sha256 {
            if !expected.eq_ignore_ascii_case(&sha256) {
                return Err(BlobError::DigestMismatch {
                    expected: expected.clone(),
                    actual: sha256,
                });
            }
        }
        let options = PutOptions {
            content_type: manifest.content_type,
            metadata: manifest.metadata,
            if_none_match: manifest.if_none_match,
            expected_sha256: manifest.expected_sha256,
        };
        let meta = self
            .commit_object(&paths, key, &body, &options, sha256)
            .await?;
        let _ = fs::remove_dir_all(&dir).await;
        Ok(meta)
    }

    async fn abort_multipart(&self, bucket: &str, _key: &ObjectKey, upload_id: &str) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        let _ = fs::remove_dir_all(paths.upload(upload_id)).await;
        Ok(())
    }
}

/// the staged state of an in-progress multipart upload.
#[derive(serde::Serialize, serde::Deserialize)]
struct UploadManifest {
    key: String,
    content_type: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default)]
    if_none_match: bool,
    #[serde(default)]
    expected_sha256: Option<String>,
}

/// zero-padded so a part directory sorts the way the parts concatenate.
fn part_filename(part_number: u32) -> String {
    format!("part-{part_number:05}")
}

/// a collision-free name for a staged file or an upload id.
fn staging_name(kind: &str) -> String {
    format!("{kind}-{}", uuid::Uuid::now_v7())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
