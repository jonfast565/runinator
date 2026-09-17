//! Batch registered pack locations and validate from bounded disposable local pack copies.
use super::workspace_objects::SharedObjects;
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    index::{DiskIndex, IndexWriter, Location},
    store::{Object, ObjectInfo, ReadStore},
};
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    sync::{Arc, Mutex},
};

const MAX_PACK_BYTES: u64 = 80 * 1024 * 1024;
const MAX_CACHED_PACKS: usize = 8;
const MAX_MEMORY_INDEX_ENTRIES: usize = 65_536;

mod memory_location;
use memory_location::MemoryLocation;

mod found_object;
use found_object::FoundObject;

mod local_pack;
use local_pack::LocalPack;

mod seal_objects;
pub(super) use seal_objects::SealObjects;

mod pack_objects;
use pack_objects::PackObjects;
