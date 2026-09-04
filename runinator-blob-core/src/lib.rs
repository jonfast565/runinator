//! the blob contract and its backend-independent pieces.
//!
//! this crate holds what every participant needs to *talk about* an object store: the [`BlobStore`]
//! trait, the key/range/metadata types, [`BlobError`], the signature-v4 implementation both ends
//! share, and the local filesystem backend. it deliberately excludes the http server and client —
//! those live in `runinator-blob`, which depends on this crate.
//!
//! Code that only reads and writes through an `Arc<dyn BlobStore>` should depend on this crate.
//! Only binaries that build a concrete store need `runinator-blob`.

pub mod errors;
pub mod fs;
pub mod key;
pub mod listing;
pub mod meta;
pub mod multipart;
pub mod range;
pub mod sigv4;
pub mod store;

pub use errors::BlobError;
pub use fs::FsBlobStore;
pub use key::{validate_bucket, ObjectKey, MAX_KEY_BYTES};
pub use listing::{BucketSummary, ListRequest, ListResponse, ObjectSummary, DEFAULT_MAX_KEYS};
pub use meta::{
    sha256_from_checksum_header, sha256_hex, sha256_hex_to_base64, ObjectBytes, ObjectMeta,
    PutOptions, DEFAULT_CONTENT_TYPE,
};
pub use multipart::{CompletedPart, MultipartUpload, MAX_PART_NUMBER, MIN_PART_NUMBER};
pub use range::{ByteRange, ResolvedRange};
pub use sigv4::{BlobCredential, CredentialStore};
pub use store::{BlobStore, ObjectReader, Result};

/// the bucket runinator stores immutable function-package artifacts in, keyed by content digest.
pub const FUNCTION_ARTIFACT_BUCKET: &str = "runinator-function-artifacts";

/// the bucket runinator stores workflow run artifacts in.
pub const RUN_ARTIFACT_BUCKET: &str = "runinator-run-artifacts";

/// the bucket holding user-uploaded workflow input files and reusable library revisions.
pub const WORKFLOW_FILE_BUCKET: &str = "runinator-workflow-files";

/// Encrypted, immutable execution-profile publications.
pub const EXECUTION_PROFILE_BUCKET: &str = "runinator-execution-profiles";

/// buckets the runinator services create before accepting work.
pub const WORKSPACE_BUCKET: &str = "runinator-workspaces";

pub const REQUIRED_BUCKETS: [&str; 5] = [
    FUNCTION_ARTIFACT_BUCKET,
    RUN_ARTIFACT_BUCKET,
    WORKFLOW_FILE_BUCKET,
    EXECUTION_PROFILE_BUCKET,
    WORKSPACE_BUCKET,
];

/// URI scheme stored for an artifact whose bytes are in a blob store.
/// Older rows contain a local filesystem path instead.
pub const BLOB_URI_SCHEME: &str = "blob";

/// build the canonical `blob://<bucket>/<key>` URI for an object.
pub fn blob_uri(bucket: &str, key: &ObjectKey) -> String {
    format!("{BLOB_URI_SCHEME}://{bucket}/{key}")
}

/// Split a `blob://<bucket>/<key>` URI into its parts.
/// Return `None` for any other shape.
pub fn parse_blob_uri(uri: &str) -> Option<(String, ObjectKey)> {
    let rest = uri.strip_prefix(&format!("{BLOB_URI_SCHEME}://"))?;
    let (bucket, key) = rest.split_once('/')?;
    validate_bucket(bucket).ok()?;
    Some((bucket.to_string(), ObjectKey::parse(key).ok()?))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
