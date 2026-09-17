#[allow(unused_imports)]
use super::*;

/// resource bounds for a filesystem-backed store.
#[derive(Clone, Debug)]
pub struct FsBlobStoreOptions {
    pub metadata_cache_bytes: usize,
    pub io_buffer_bytes: usize,
    pub max_concurrent_writes: usize,
}

impl Default for FsBlobStoreOptions {
    fn default() -> Self {
        Self {
            metadata_cache_bytes: DEFAULT_METADATA_CACHE_BYTES,
            io_buffer_bytes: DEFAULT_IO_BUFFER_BYTES,
            max_concurrent_writes: DEFAULT_MAX_CONCURRENT_WRITES,
        }
    }
}
