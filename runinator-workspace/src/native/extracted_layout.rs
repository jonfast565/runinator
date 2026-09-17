#[allow(unused_imports)]
use super::*;

pub(super) struct ExtractedLayout {
    pub(super) directory: tempfile::TempDir,
    pub(super) manifest: serde_json::Value,
    pub(super) transfer_limit: u64,
}
