//! Immutable reads independent of publication, transport, and retention policy.

use crate::{
    Id, Result,
    cache::ByteCache,
    codec::Binary,
    error::invalid,
    model::{FileObject, Inode, InodeData, Kind, Revision, Workspace},
    pages,
    projection::{PathNode, PathProjection},
    radix,
    store::{ReadStore, load, load_many},
};

mod view;
pub use view::View;

mod entry;
pub use entry::Entry;
