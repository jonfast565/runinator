//! the blob contract.

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::errors::BlobError;
use crate::key::ObjectKey;
use crate::listing::{BucketSummary, ListRequest, ListResponse};
use crate::meta::{ObjectBytes, ObjectMeta, PutOptions};
use crate::multipart::CompletedPart;
use crate::range::{ByteRange, ResolvedRange};

pub type Result<T> = std::result::Result<T, BlobError>;

/// an object's bytes as a stream, for callers that must not hold the whole object in memory.
pub struct ObjectReader {
    pub meta: ObjectMeta,
    /// present when the read was ranged; `None` means the reader covers the whole object.
    pub range: Option<ResolvedRange>,
    pub body: Box<dyn AsyncRead + Send + Unpin>,
}

impl ObjectReader {
    /// how many bytes this reader will yield.
    pub fn len(&self) -> u64 {
        self.range
            .map(|range| range.length)
            .unwrap_or(self.meta.size)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// a content-addressed object store with random-access reads.
///
/// implementations are addressed by `(bucket, key)` and are expected to be safe to share across
/// tasks. the trait is object-safe on purpose: consumers hold `Arc<dyn BlobStore>` so a deployment
/// can swap the filesystem backend for the http one without a generic parameter reaching through
/// every call site.
#[async_trait]
pub trait BlobStore: Send + Sync + 'static {
    /// a short name for the backing implementation, for logs and health output.
    fn backend(&self) -> &'static str;

    async fn create_bucket(&self, bucket: &str) -> Result<()>;

    async fn delete_bucket(&self, bucket: &str) -> Result<()>;

    async fn bucket_exists(&self, bucket: &str) -> Result<bool>;

    async fn list_buckets(&self) -> Result<Vec<BucketSummary>>;

    async fn put(
        &self,
        bucket: &str,
        key: &ObjectKey,
        body: Vec<u8>,
        options: PutOptions,
    ) -> Result<ObjectMeta>;

    async fn head(&self, bucket: &str, key: &ObjectKey) -> Result<ObjectMeta>;

    /// open a (possibly ranged) reader over an object.
    async fn open(
        &self,
        bucket: &str,
        key: &ObjectKey,
        range: Option<ByteRange>,
    ) -> Result<ObjectReader>;

    async fn delete(&self, bucket: &str, key: &ObjectKey) -> Result<()>;

    async fn list(&self, bucket: &str, request: &ListRequest) -> Result<ListResponse>;

    async fn create_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        options: PutOptions,
    ) -> Result<String>;

    async fn upload_part(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        body: Vec<u8>,
    ) -> Result<String>;

    async fn complete_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<ObjectMeta>;

    async fn abort_multipart(&self, bucket: &str, key: &ObjectKey, upload_id: &str) -> Result<()>;

    /// read an object (or a range of it) fully into memory.
    ///
    /// provided rather than required because every backend can answer it from `open`, and a
    /// backend-specific shortcut would be a second code path to keep honest.
    async fn get(
        &self,
        bucket: &str,
        key: &ObjectKey,
        range: Option<ByteRange>,
    ) -> Result<ObjectBytes> {
        let mut reader = self.open(bucket, key, range).await?;
        let mut data = Vec::with_capacity(reader.len() as usize);
        reader
            .body
            .read_to_end(&mut data)
            .await
            .map_err(|err| BlobError::Io(format!("reading {bucket}/{key}: {err}")))?;
        Ok(ObjectBytes {
            meta: reader.meta,
            range: reader.range,
            data,
        })
    }

    /// true when the key exists. provided so callers do not each rewrite the not-found match.
    async fn exists(&self, bucket: &str, key: &ObjectKey) -> Result<bool> {
        match self.head(bucket, key).await {
            Ok(_) => Ok(true),
            Err(BlobError::NotFound(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }
}
