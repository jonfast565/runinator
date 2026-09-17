#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct UnpackLimits {
    pub(super) entries: usize,
    pub(super) entry_bytes: u64,
    pub(super) total_bytes: u64,
}

impl Default for UnpackLimits {
    fn default() -> Self {
        Self {
            entries: MAX_ARCHIVE_ENTRIES,
            entry_bytes: MAX_UNPACKED_ENTRY_BYTES,
            total_bytes: MAX_UNPACKED_ARCHIVE_BYTES,
        }
    }
}
