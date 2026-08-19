//! the blob service and its client.
//!
//! this crate holds the concrete transports over [`runinator_blob_core`]: an s3-compatible http
//! server, an http client that implements the same [`BlobStore`] trait, and the factory that picks
//! one from configuration. it re-exports the core surface, so a binary that *builds* a store needs
//! only this crate.
//!
//! a crate that merely reads and writes through an `Arc<dyn BlobStore>` should depend on
//! `runinator-blob-core` instead — that is what keeps the axum and reqwest dependency surface
//! confined to the binaries that actually assemble a store.
//!
//! ## what "minimally s3 compatible" means here
//!
//! path-style addressing, aws signature v4 (header and presigned-query), object put/get/head/delete
//! with ranged reads, bucket create/head/delete/list, `ListObjectsV2`, and multipart upload. that is
//! enough for the aws cli and the aws sdks to drive it, which is the point: the same code paths work
//! against real s3 or minio later with only configuration changing.
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
};
