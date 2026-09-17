#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct InventoryEntry {
    pub(super) inode_number: u64,
    pub(super) inode: Inode,
}
