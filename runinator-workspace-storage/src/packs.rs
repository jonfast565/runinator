//! Bounded pack production for shared storage and native transfers.

use crate::{Id, Result, disk::PackBuilder, gc, index::DiskIndex, store::ReadStore};
use std::path::{Path, PathBuf};

pub const TARGET_PACK_BYTES: u64 = 64 * 1024 * 1024;

pub struct Pack {
    pub id: Id,
    pub path: PathBuf,
    pub index: DiskIndex,
    _index_file: tempfile::NamedTempFile,
}

/// Emit the selected revision's reachable objects, omitting those already in the base.
/// The callback must consume each pack before returning; no revision ref is published.
pub fn seal<S: ReadStore, B: ReadStore, F: FnMut(Pack) -> Result<()>>(
    store: &S,
    base: &B,
    revision: Id,
    scratch: &Path,
    emit: F,
) -> Result<()> {
    seal_roots(store, base, &[revision], scratch, emit)
}

pub fn seal_roots<S: ReadStore, B: ReadStore, F: FnMut(Pack) -> Result<()>>(
    store: &S,
    base: &B,
    revisions: &[Id],
    scratch: &Path,
    mut emit: F,
) -> Result<()> {
    let directory = tempfile::tempdir_in(scratch)?;
    let root = directory.path();
    std::fs::create_dir(root.join("packs"))?;
    std::fs::create_dir(root.join("tmp"))?;
    let marks = gc::mark_roots(store, revisions, root, false)?;
    let mut builder = Some(PackBuilder::new(root)?);
    fn flush<F: FnMut(Pack) -> Result<()>>(
        builder: PackBuilder,
        root: &Path,
        emit: &mut F,
    ) -> Result<()> {
        let sealed = builder.finish(root)?;
        if let Some(id) = sealed.pack {
            let path = root.join("packs").join(format!("{id}.pack"));
            let index = DiskIndex::open(sealed.index.path())?;
            emit(Pack {
                id,
                path: path.clone(),
                index,
                _index_file: sealed.index,
            })?;
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    marks.visit(|id| {
        if base.contains(id)? {
            return Ok(());
        }
        let object = store.get(id)?;
        let current = builder.as_mut().ok_or(crate::Error::Poisoned)?;
        current.add(object.kind, &object.bytes)?;
        if current.encoded_bytes()? >= TARGET_PACK_BYTES - 5 * 1024 * 1024 {
            flush(
                builder.take().ok_or(crate::Error::Poisoned)?,
                root,
                &mut emit,
            )?;
            builder = Some(PackBuilder::new(root)?);
        }
        Ok(())
    })?;
    flush(
        builder.take().ok_or(crate::Error::Poisoned)?,
        root,
        &mut emit,
    )
}
