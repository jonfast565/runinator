#[allow(unused_imports)]
use super::*;

pub(super) struct ActiveWorkerSettings {
    pub(super) config_hash: String,
    pub(super) max_concurrent_actions: u64,
    pub(super) shutdown_grace_seconds: u64,
    pub(super) source: String,
}
