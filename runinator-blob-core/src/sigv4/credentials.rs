//! the static credentials both ends sign with.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::BlobError;

/// the default region token. nothing here is region-aware, but sigv4's credential scope requires
/// one, and a mismatch between client and server is a signature failure, so it is pinned.
pub const DEFAULT_REGION: &str = "us-east-1";

/// the service token in the credential scope. `S3` so an unmodified aws sdk signs correctly.
pub const SERVICE: &str = "s3";

mod blob_credential;
pub use blob_credential::BlobCredential;

mod credential_store;
pub use credential_store::CredentialStore;
