#[allow(unused_imports)]
use super::*;

pub(super) struct Spool {
    pub(super) file: tokio::fs::File,
    pub(super) path: tempfile::TempPath,
    pub(super) size: u64,
    pub(super) sha256: String,
}
