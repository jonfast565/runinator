#[allow(unused_imports)]
use super::*;

pub(super) struct FileState {
    pub(super) entries: VecDeque<OutboxEntry>,
    pub(super) bytes: u64,
}
