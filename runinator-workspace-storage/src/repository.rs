use crate::{
    Error, Id,
    cache::{ByteCache, ObjectCaches},
    catalog::{self, Catalog},
    disk::{self, DiskView, OverlayStore, PackBuilder, RawDisk},
    error::{Result, invalid},
    gc::{self, GcReport},
    index::{self, DiskIndex},
    io_util,
    model::{Inode, InodeData, Kind, Layout, Revision, Workspace},
    namespace, pages,
    store::{Object, ObjectInfo, ReadStore, load},
};
use rayon::prelude::*;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock, RwLockReadGuard,
        atomic::{AtomicU8, Ordering},
    },
};

/// Fault injection is opt-in and one-shot. It is for testing recovery, not a
/// substitute for power-cut/filesystem fault testing on the deployment platform.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum CommitPoint {
    AfterPack = 1,
    AfterIndex = 2,
    BeforeCurrent = 3,
    AfterCurrent = 4,
}

mod config;
pub use config::Config;

mod state;
use state::State;

mod repository_stats;
pub use repository_stats::RepositoryStats;

mod repository;
pub use repository::Repository;

mod snapshot;
pub use snapshot::Snapshot;

mod transaction;
pub use transaction::Transaction;
