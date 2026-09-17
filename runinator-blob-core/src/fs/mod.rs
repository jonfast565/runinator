//! the local filesystem backend.

mod cache;
mod format;
mod migrate;
mod paths;
mod walk;

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};

use crate::errors::BlobError;
use crate::key::{validate_bucket, ObjectKey};
use crate::listing::{BucketSummary, ListRequest, ListResponse};
use crate::meta::{ObjectMeta, PutOptions, DEFAULT_CONTENT_TYPE};
use crate::multipart::{CompletedPart, MAX_PART_NUMBER, MIN_PART_NUMBER};
use crate::range::ByteRange;
use crate::store::{BlobStore, ObjectReader, Result};

use cache::MetadataCache;
use format::{PartMeta, OBJECT_MAGIC, PART_MAGIC};
use paths::BucketPaths;

pub const DEFAULT_METADATA_CACHE_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_IO_BUFFER_BYTES: usize = 256 * 1024;
pub const DEFAULT_MAX_CONCURRENT_WRITES: usize = 8;
const KEY_LOCK_SHARDS: usize = 256;

/// an object store backed by a directory tree owned exclusively by this process.

fn part_filename(part_number: u32) -> String {
    format!("part-{part_number:05}")
}

fn staging_name(kind: &str) -> String {
    format!("{kind}-{}", uuid::Uuid::now_v7())
}

async fn load_buckets(root: &Path) -> Result<HashMap<String, DateTime<Utc>>> {
    let mut buckets = HashMap::new();
    let mut entries = fs::read_dir(root)
        .await
        .map_err(|err| BlobError::Io(format!("listing {}: {err}", root.display())))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| BlobError::Io(format!("listing {}: {err}", root.display())))?
    {
        let marker = entry.path().join(paths::BUCKET_MARKER);
        if !fs::try_exists(&marker).await.unwrap_or(false) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        buckets.insert(name, marker_created_at(&marker).await);
    }
    Ok(buckets)
}

async fn marker_created_at(marker: &Path) -> DateTime<Utc> {
    fs::metadata(marker)
        .await
        .and_then(|meta| meta.created())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "benchmark_tests.rs"]
mod benchmarks;

mod fs_blob_store_options;
pub use fs_blob_store_options::FsBlobStoreOptions;

mod fs_blob_store;
pub use fs_blob_store::FsBlobStore;

mod upload_manifest;
use upload_manifest::UploadManifest;
