#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub inode_number: u64,
    pub inode_id: Id,
    pub inode: Inode,
    pub size: u64,
}
