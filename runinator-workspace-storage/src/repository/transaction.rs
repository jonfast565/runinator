#[allow(unused_imports)]
use super::*;

pub struct Transaction<'a> {
    pub(super) _guard: RwLockReadGuard<'a, ()>,
    pub(super) repository: &'a Repository,
    pub(super) name: String,
    pub(crate) expected: Option<Id>,
    pub(super) core: crate::transaction::Edit<OverlayStore>,
}

impl Transaction<'_> {
    pub fn commit(self, message: &str) -> Result<Id> {
        self.commit_with_parent(message, true)
    }
    pub(super) fn commit_with_parent(self, message: &str, keep_parent: bool) -> Result<Id> {
        let id = self
            .core
            .finish(message, if keep_parent { self.expected } else { None })?;
        self.publish_revision(id)
    }
    pub(crate) fn publish_revision(self, id: Id) -> Result<Id> {
        let repo = self.repository;
        let mut state = repo.state.lock().map_err(|_| Error::Poisoned)?;
        Repository::ensure(&state)?;
        if state.catalog.refs.get(&self.name).copied() != self.expected {
            return Err(Error::Conflict);
        }
        let sealed = disk::seal(&self.store, &repo.raw(&state), id)?;
        repo.hit(CommitPoint::AfterPack)?;
        let staged_index = DiskIndex::open(sealed.index.path())?;
        let merged = index::merge(&state.index, &staged_index, &repo.root.join("tmp"))?;
        let new_index = DiskIndex::open(merged.path())?;
        let index_id = io_util::install(merged, &repo.root.join("indexes"), ".idx")?;
        repo.hit(CommitPoint::AfterIndex)?;
        let mut catalog = state.catalog.clone();
        catalog.generation = catalog
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("generation overflow"))?;
        if let Some(pack) = sealed.pack {
            catalog.packs.insert(pack);
        }
        catalog.index = Some(index_id);
        catalog.refs.insert(self.name.clone(), id);
        repo.publish(&mut state, catalog, new_index)?;
        Ok(id)
    }
}

impl std::ops::Deref for Transaction<'_> {
    type Target = crate::transaction::Edit<OverlayStore>;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl std::ops::DerefMut for Transaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}
