#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(super) struct RuntimeLimits {
    pub(super) memory_mb: i64,
    pub(super) cpu_millis: i64,
    pub(super) pids: i64,
    pub(super) tmpfs_mb: i64,
    pub(super) max_output_bytes: usize,
}
