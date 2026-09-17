#[allow(unused_imports)]
use super::*;

/// Request the process's object store and optionally reconcile its buckets on startup.
#[derive(Debug, Clone, Copy)]
pub struct BlobRequest {
    pub ensure_buckets: bool,
}
