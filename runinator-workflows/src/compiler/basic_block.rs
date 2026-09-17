#[allow(unused_imports)]
use super::*;

pub(super) struct BasicBlock {
    pub(super) label: Label,
    pub(super) node_id: String,
    pub(super) instructions: Vec<PendingInstruction>,
    /// Whether an interrupt may suspend a thread positioned here.
    pub(super) interruptible: bool,
    /// Offset within the block of its trailing exit sequence, when it has one.
    pub(super) exit_offset: Option<usize>,
}
