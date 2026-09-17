use crate::{
    Id,
    codec::{Binary, Decoder, Encoder},
    error::{Result, corrupt, invalid},
};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Chunk = 1,
    Page = 2,
    Radix = 3,
    File = 4,
    Inode = 5,
    Link = 6,
    Workspace = 7,
    Revision = 8,
    PathProjection = 9,
    PathNode = 10,
    PathList = 11,
    PathRef = 12,
    RefList = 13,
    /// Physical-only container; never a logical Merkle edge.
    TinyBlock = 14,
    /// Physical-only compression group for logical Chunk objects.
    ChunkBlock = 15,
    /// Physical-only container for small logical metadata objects.
    MetadataBlock = 16,
}
impl TryFrom<u8> for Kind {
    type Error = crate::Error;
    fn try_from(v: u8) -> Result<Self> {
        Ok(match v {
            1 => Self::Chunk,
            2 => Self::Page,
            3 => Self::Radix,
            4 => Self::File,
            5 => Self::Inode,
            6 => Self::Link,
            7 => Self::Workspace,
            8 => Self::Revision,
            9 => Self::PathProjection,
            10 => Self::PathNode,
            11 => Self::PathList,
            12 => Self::PathRef,
            13 => Self::RefList,
            14 => Self::TinyBlock,
            15 => Self::ChunkBlock,
            16 => Self::MetadataBlock,
            _ => return Err(corrupt("unknown object kind")),
        })
    }
}

/// Representation limits are format constants, not per-process heuristics.
pub const INLINE_LIMIT: usize = 128;
pub const TINY_LIMIT: usize = 64 * 1024;
pub const ZERO_RUN_MIN: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PageExtent {
    Data(ChunkRef),
    Zero(u32),
}
impl PageExtent {
    pub fn len(&self) -> u32 {
        match self {
            Self::Data(c) => c.len,
            Self::Zero(n) => *n,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InodeData {
    File(Id),
    Directory(Option<Id>),
    Symlink(String),
}

mod layout;
pub use layout::Layout;

mod chunk_ref;
pub use chunk_ref::ChunkRef;

mod page;
pub use page::Page;

mod radix_node;
pub use radix_node::RadixNode;

mod file_object;
pub use file_object::FileObject;

mod metadata;
pub use metadata::Metadata;

mod inode;
pub use inode::Inode;

mod link;
pub use link::Link;

mod workspace;
pub use workspace::Workspace;

mod revision;
pub use revision::Revision;
