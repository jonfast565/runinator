#[allow(unused_imports)]
use super::*;

pub(super) struct SimLoopFrame {
    pub(super) items: Vec<Value>,
    pub(super) index: usize,
    pub(super) results: Vec<Value>,
}
