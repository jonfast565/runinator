//! the blob service and its client.
//!
//! This crate holds the concrete transports over [`runinator_blob_core`]: an S3-compatible HTTP
//! server, an HTTP client that implements the same [`BlobStore`] trait, and a configuration factory.
//! It re-exports the core surface, so a binary that *builds* a store needs
//! only this crate.
//!
//! a crate that merely reads and writes through an `Arc<dyn BlobStore>` should depend on
//! `runinator-blob-core` instead — that is what keeps the axum and reqwest dependency surface
//! confined to the binaries that actually assemble a store.
//!
//! ## what "minimally S3 compatible" means here
//!
//! It supports path-style addressing, AWS Signature V4 headers and presigned queries, object
//! operations with ranged reads, bucket operations, `ListObjectsV2`, and multipart uploads. AWS
//! CLI and SDK clients can use it. Switching to real S3 or MinIO only needs configuration changes.
//!
//! deliberately absent: virtual-host addressing, versioning, lifecycle, acls, cors, tagging,
//! server-side encryption, and `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` chunk signing. an etag here is a
//! quoted sha-256, not an md5.

#[cfg(feature = "client")]
pub mod client;
pub mod config;
#[cfg(feature = "client")]
pub mod factory;
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub use client::S3BlobClient;
pub use config::BlobClientConfig;
#[cfg(feature = "server")]
pub use config::BlobServerConfig;
#[cfg(feature = "client")]
pub use factory::{ensure_buckets, from_env};
#[cfg(feature = "server")]
pub use server::{router, run_server};

// the contract, re-exported at its historical path so a binary that builds a store names one crate.
pub use runinator_blob_core::{
    blob_uri, errors, key, listing, meta, multipart, parse_blob_uri, range, sha256_hex, sigv4,
    store, BlobCredential, BlobError, BlobStore, ByteRange, CredentialStore, FsBlobStore,
    ListRequest, ListResponse, ObjectBytes, ObjectKey, ObjectMeta, ObjectReader, ObjectSummary,
    PutOptions, ResolvedRange, BLOB_URI_SCHEME, FUNCTION_ARTIFACT_BUCKET, RUN_ARTIFACT_BUCKET,
    WORKFLOW_FILE_BUCKET,
};
