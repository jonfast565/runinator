#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct State {
    pub(super) entries: HashMap<Id, Entry>,
    pub(super) clock: VecDeque<Id>,
    pub(super) pending: HashSet<Id>,
    pub(super) used: usize,
    pub(super) reserved: usize,
    pub(super) hits: u64,
    pub(super) misses: u64,
}
