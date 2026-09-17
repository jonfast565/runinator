#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct WalkOptions {
    pub include_ancestors: bool,
    pub load_chunks: bool,
    pub batch_size: usize,
}
