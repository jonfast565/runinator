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
mod object_reader;
pub use object_reader::ObjectReader;

mod blob_store;
pub use blob_store::BlobStore;
