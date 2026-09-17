use crate::{
    Error, Id,
    cache::{ByteCache, ObjectCaches},
    error::{Result, corrupt, invalid},
    index::{DiskIndex, ExternalSorter, Location, STANDALONE},
    io_util,
    model::Kind,
    record,
    store::{Object, ObjectInfo, ReadStore, WriteStore},
    tiny,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::{NamedTempFile, TempDir};

/// Unpublished objects spill immediately to transaction-local disk files, not
/// an unbounded HashMap. A failed operation can leave harmless staged garbage.

pub(crate) fn seal(overlay: &OverlayStore, latest: &RawDisk, revision: Id) -> Result<SealedPack> {
    let mut builder = PackBuilder::new(&latest.root)?;
    let marks = crate::gc::mark_roots(overlay, &[revision], &latest.root.join("tmp"), true)?;
    marks.visit(|id| {
        if !latest.contains(id)? {
            let object = overlay.get(id)?;
            builder.add(object.kind, &object.bytes)?;
        }
        Ok(())
    })?;
    builder.finish(&latest.root)
}

mod raw_disk;
pub use raw_disk::RawDisk;

mod disk_view;
pub use disk_view::DiskView;

mod overlay_store;
pub use overlay_store::OverlayStore;

mod raw_overlay;
use raw_overlay::RawOverlay;

mod pack_builder;
pub(crate) use pack_builder::PackBuilder;

mod sealed_pack;
pub(crate) use sealed_pack::SealedPack;
