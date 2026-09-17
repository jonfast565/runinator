//! Application service for staged and reusable workflow input files.
//!
//! File bytes are written exactly once to the shared blob store. The store role records the
//! authorization and lifecycle metadata that lets the VM retain a portable descriptor instead of
//! a legacy run-artifact row.

use std::sync::Arc;

use chrono::Utc;
use runinator_blob_core::{
    BlobStore, ObjectKey, PutOptions, WORKFLOW_FILE_BUCKET, blob_uri, sha256_hex,
};
use runinator_models::{
    errors::SendableError,
    files::{FileDescriptor, FileScope, StoredFile, validate_relative_path},
};
use runinator_store::{RuntimeStore, roles::FileStore};
use uuid::Uuid;

/// Kept private to this service so the handler can stream without importing the generic artifact
/// storage module (whose run-artifact semantics should remain separate).
pub mod runinator_engine_file_content {
    pub struct Content {
        pub size_bytes: u64,
        pub body: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    }
}

mod workflow_files;
pub use workflow_files::WorkflowFiles;

mod new_stored_file;
use new_stored_file::NewStoredFile;
