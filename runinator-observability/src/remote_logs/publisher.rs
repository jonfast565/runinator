#[allow(unused_imports)]
use super::*;

pub(super) struct Publisher {
    pub(super) sender: SyncSender<RuntimeLogRecord>,
    pub(super) queued_bytes: Arc<AtomicUsize>,
    pub(super) dropped: Arc<AtomicU64>,
    pub(super) source: String,
}
