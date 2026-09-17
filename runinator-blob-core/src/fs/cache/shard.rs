#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct Shard {
    pub(super) entries: HashMap<String, Entry>,
    pub(super) order: VecDeque<(String, u64)>,
    pub(super) used: usize,
    pub(super) generation: u64,
}
