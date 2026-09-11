//! Bounded pack production for shared storage and native transfers.

use crate::{
    Id, Result, disk::PackBuilder, gc, index::DiskIndex, staging::StagedStore, store::ReadStore,
};
use std::path::{Path, PathBuf};

pub const TARGET_PACK_BYTES: u64 = 64 * 1024 * 1024;
const FETCH_BATCH: usize = 16;

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

/// Emit only reachable objects created by the current staged edit.
///
/// Unstaged edges belong to the immutable base and are pruned before they are loaded. The callback
/// must consume each pack before returning; no revision ref is published.
pub fn seal_staged<S: StagedStore, B: ReadStore, F: FnMut(Pack) -> Result<()>>(
    store: &S,
    base: &B,
    revision: Id,
    scratch: &Path,
    mut emit: F,
) -> Result<()> {
    seal_filtered(
        store,
        base,
        &[revision],
        scratch,
        |id| store.is_staged(id),
        &mut emit,
    )
}

pub fn seal_roots<S: ReadStore, B: ReadStore, F: FnMut(Pack) -> Result<()>>(
    store: &S,
    base: &B,
    revisions: &[Id],
    scratch: &Path,
    mut emit: F,
) -> Result<()> {
    seal_filtered(store, base, revisions, scratch, |_| Ok(true), &mut emit)
}

fn seal_filtered<S, B, P, F>(
    store: &S,
    base: &B,
    revisions: &[Id],
    scratch: &Path,
    select: P,
    mut emit: F,
) -> Result<()>
where
    S: ReadStore,
    B: ReadStore,
    P: FnMut(Id) -> Result<bool>,
    F: FnMut(Pack) -> Result<()>,
{
    let directory = tempfile::tempdir_in(scratch)?;
    let root = directory.path();
    std::fs::create_dir(root.join("packs"))?;
    std::fs::create_dir(root.join("tmp"))?;
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
    fn add_batch<B: ReadStore, F: FnMut(Pack) -> Result<()>>(
        base: &B,
        ids: &[Id],
        objects: &[crate::store::Object],
        builder: &mut Option<PackBuilder>,
        root: &Path,
        emit: &mut F,
    ) -> Result<()> {
        let mut missing = Vec::with_capacity(ids.len());
        for (&id, object) in ids.iter().zip(objects) {
            if !base.contains(id)? {
                missing.push(object.clone());
            }
        }
        for objects in missing.chunks(FETCH_BATCH) {
            let current = builder.as_mut().ok_or(crate::Error::Poisoned)?;
            current.add_many(objects)?;
            if current.encoded_bytes()? >= TARGET_PACK_BYTES - 5 * 1024 * 1024 {
                flush(builder.take().ok_or(crate::Error::Poisoned)?, root, emit)?;
                *builder = Some(PackBuilder::new(root)?);
            }
        }
        Ok(())
    }
    gc::walk_roots_filtered(
        store,
        revisions,
        root,
        gc::WalkOptions {
            include_ancestors: false,
            load_chunks: true,
            batch_size: FETCH_BATCH,
        },
        select,
        |ids, objects| add_batch(base, ids, objects, &mut builder, root, &mut emit),
    )?;
    flush(
        builder.take().ok_or(crate::Error::Poisoned)?,
        root,
        &mut emit,
    )
}
