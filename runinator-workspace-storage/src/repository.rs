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
#[derive(Clone, Debug)]
pub struct Config {
    pub layout: Layout,
    pub page_cache_bytes: usize,
    pub metadata_cache_bytes: usize,
    pub chunk_cache_bytes: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            page_cache_bytes: 64 * 1024 * 1024,
            metadata_cache_bytes: 16 * 1024 * 1024,
            chunk_cache_bytes: 32 * 1024 * 1024,
        }
    }
}
impl Config {
    fn validate(&self) -> Result<()> {
        self.layout.validate()?;
        if self.page_cache_bytes < self.layout.page_size as usize
            || self.metadata_cache_bytes < crate::model::TINY_LIMIT + 128
            || self.chunk_cache_bytes < self.layout.max as usize
        {
            return Err(invalid("cache budgets are too small for this layout"));
        }
        Ok(())
    }
}
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
struct State {
    digest: Id,
    catalog: Catalog,
    index: DiskIndex,
    uncertain: bool,
}
#[derive(Clone, Debug)]
pub struct RepositoryStats {
    pub generation: u64,
    pub objects: u64,
    pub packs: usize,
    pub refs: usize,
}
pub struct Repository {
    pub(crate) root: PathBuf,
    _process_lock: File,
    // One process owns a repository. Within that process many threads may use
    // snapshots and optimistic transactions concurrently through Arc<Repository>.
    gate: RwLock<()>,
    state: Mutex<State>,
    pub(crate) config: Config,
    pub(crate) pages: ByteCache,
    caches: Arc<ObjectCaches>,
    tiny_blocks: Arc<ByteCache>,
    failpoint: AtomicU8,
}
impl Repository {
    pub fn init(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        Self::open_impl(path.as_ref(), config, true)
    }
    pub fn open(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        Self::open_impl(path.as_ref(), config, false)
    }
    fn open_impl(path: &Path, config: Config, create: bool) -> Result<Self> {
        config.validate()?;
        if !cfg!(unix) {
            return Err(invalid(
                "durable disk repositories currently require Unix local-filesystem fsync/rename semantics",
            ));
        }
        if create {
            fs::create_dir_all(path)?;
        }
        let root = fs::canonicalize(path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("LOCK"))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Error::Busy("repository is open in another process/handle".into())
            } else {
                e.into()
            }
        })?;
        if create {
            if root.join("CURRENT").try_exists()? {
                return Err(Error::Exists("repository already initialized".into()));
            }
            io_util::make_dirs(&root)?;
            let digest = catalog::write(&root, &Catalog::default())?;
            io_util::atomic_replace(&root.join("CURRENT"), format!("{digest}\n").as_bytes())?;
        }
        let (digest, catalog, index) = catalog::load(&root)?;
        let pages = ByteCache::new(config.page_cache_bytes);
        let caches = Arc::new(ObjectCaches::new(
            config.metadata_cache_bytes,
            config.chunk_cache_bytes,
        ));
        let repository = Self {
            root,
            _process_lock: lock,
            gate: RwLock::new(()),
            state: Mutex::new(State {
                digest,
                catalog,
                index,
                uncertain: false,
            }),
            config,
            pages,
            caches,
            tiny_blocks: Arc::new(ByteCache::new(crate::tiny::BLOCK_CACHE_BYTES)),
            failpoint: AtomicU8::new(0),
        };
        // The process lock proves no old transaction can still own these temps.
        for entry in fs::read_dir(repository.root.join("tmp"))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(repository)
    }
    fn ensure(state: &State) -> Result<()> {
        if state.uncertain {
            return Err(Error::CommitUncertain("close and reopen repository".into()));
        }
        Ok(())
    }
    fn raw(&self, state: &State) -> RawDisk {
        RawDisk {
            root: self.root.clone(),
            index: state.index.clone(),
            tiny_blocks: self.tiny_blocks.clone(),
        }
    }
    fn view(&self, state: &State) -> DiskView {
        DiskView {
            raw: self.raw(state),
            caches: self.caches.clone(),
        }
    }
    pub fn stats(&self) -> Result<RepositoryStats> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        Ok(RepositoryStats {
            generation: state.catalog.generation,
            objects: state.index.count,
            packs: state.catalog.packs.len(),
            refs: state.catalog.refs.len(),
        })
    }
    pub fn head(&self, name: &str) -> Result<Option<Id>> {
        catalog::validate_ref(name)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        Ok(state.catalog.refs.get(name).copied())
    }
    pub fn refs(&self) -> Result<BTreeMap<String, Id>> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        Ok(state.catalog.refs.clone())
    }
    pub fn page_cache(&self) -> &ByteCache {
        &self.pages
    }
    pub fn inject_failure_once(&self, point: CommitPoint) {
        self.failpoint.store(point as u8, Ordering::SeqCst);
    }
    fn hit(&self, point: CommitPoint) -> Result<()> {
        if self
            .failpoint
            .compare_exchange(point as u8, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Err(std::io::Error::other(format!("injected failure at {point:?}")).into());
        }
        Ok(())
    }
    fn publish(&self, state: &mut State, catalog: Catalog, index: DiskIndex) -> Result<()> {
        let digest = catalog::write(&self.root, &catalog)?;
        self.hit(CommitPoint::BeforeCurrent)?;
        let result =
            io_util::atomic_replace(&self.root.join("CURRENT"), format!("{digest}\n").as_bytes());
        if let Err(error) = result {
            // rename may have succeeded before a directory fsync failed. Never
            // continue using the old in-memory generation on an uncertain result.
            match catalog::load(&self.root) {
                Ok((digest, catalog, index)) => {
                    *state = State {
                        digest,
                        catalog,
                        index,
                        uncertain: false,
                    }
                }
                Err(_) => state.uncertain = true,
            }
            return Err(Error::CommitUncertain(error.to_string()));
        }
        *state = State {
            digest,
            catalog,
            index,
            uncertain: false,
        };
        self.hit(CommitPoint::AfterCurrent)
            .map_err(|e| Error::CommitUncertain(e.to_string()))
    }
    pub fn snapshot(&self, name: &str) -> Result<Snapshot<'_>> {
        catalog::validate_ref(name)?;
        let guard = self.gate.read().map_err(|_| Error::Poisoned)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        let id = state
            .catalog
            .refs
            .get(name)
            .copied()
            .ok_or_else(|| Error::NotFound(name.into()))?;
        self.make_snapshot(id, guard, &state)
    }
    pub fn snapshot_at(&self, id: Id) -> Result<Snapshot<'_>> {
        let guard = self.gate.read().map_err(|_| Error::Poisoned)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        self.make_snapshot(id, guard, &state)
    }
    fn make_snapshot<'a>(
        &'a self,
        id: Id,
        guard: RwLockReadGuard<'a, ()>,
        state: &State,
    ) -> Result<Snapshot<'a>> {
        let store = self.view(state);
        let revision: Revision = load(&store, id, Kind::Revision)?;
        let workspace = load(&store, revision.workspace, Kind::Workspace)?;
        Ok(Snapshot {
            _guard: guard,
            repository: self,
            store,
            id,
            revision,
            workspace,
        })
    }
    pub fn transaction(&self, name: &str) -> Result<Transaction<'_>> {
        catalog::validate_ref(name)?;
        let guard = self.gate.read().map_err(|_| Error::Poisoned)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        let expected = state.catalog.refs.get(name).copied();
        let store = OverlayStore::new(self.raw(&state), self.caches.clone())?;
        drop(state);
        let core = crate::transaction::Edit::new(
            store,
            expected,
            self.config.layout,
            self.config.page_cache_bytes,
        )?;
        Ok(Transaction {
            _guard: guard,
            repository: self,
            name: name.into(),
            expected,
            core,
        })
    }
    /// Atomic expected-old comparison. None means the ref must not exist.
    pub fn update_ref_if(&self, name: &str, expected: Option<Id>, new: Option<Id>) -> Result<()> {
        catalog::validate_ref(name)?;
        let _gate = self.gate.read().map_err(|_| Error::Poisoned)?;
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        if state.catalog.refs.get(name).copied() != expected {
            return Err(Error::Conflict);
        }
        if let Some(id) = new {
            let _: Revision = load(&self.raw(&state), id, Kind::Revision)?;
        }
        if expected == new {
            return Ok(());
        }
        let mut next = state.catalog.clone();
        next.generation = next
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("generation overflow"))?;
        match new {
            Some(id) => {
                next.refs.insert(name.into(), id);
            }
            None => {
                next.refs.remove(name);
            }
        }
        let index = state.index.clone();
        self.publish(&mut state, next, index)
    }
    /// A parentless revision is an explicit history-retention boundary. Existing
    /// branches/tags still retain anything reachable from their own roots.
    pub fn checkpoint(&self, name: &str, message: &str) -> Result<Id> {
        self.transaction(name)?.commit_with_parent(message, false)
    }
    pub fn fsck(&self) -> Result<u64> {
        let _gate = self.gate.read().map_err(|_| Error::Poisoned)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        let raw = self.raw(&state);
        let catalog = state.catalog.clone();
        drop(state);
        if let Some(id) = catalog.index {
            io_util::verify_file(&self.root.join("indexes").join(format!("{id}.idx")), id)?;
        }
        for pack in &catalog.packs {
            io_util::verify_file(&self.root.join("packs").join(format!("{pack}.pack")), *pack)?;
        }
        raw.index.validate()?;
        let roots: Vec<_> = catalog.refs.values().copied().collect();
        Ok(gc::verify_graph(&raw, &roots, &self.root.join("tmp"))?.count)
    }
    pub fn gc(&self) -> Result<GcReport> {
        let _gate = self
            .gate
            .try_write()
            .map_err(|_| Error::Busy("drop active snapshots/transactions before GC".into()))?;
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Self::ensure(&state)?;
        let raw = self.raw(&state);
        let roots: Vec<_> = state.catalog.refs.values().copied().collect();
        let marks = gc::mark(&raw, &roots, &self.root.join("tmp"))?;
        let mut builder = PackBuilder::new(&self.root)?;
        marks.visit(|id| {
            let obj = raw.get(id)?;
            builder.add(obj.kind, &obj.bytes)?;
            Ok(())
        })?;
        let sealed = builder.finish(&self.root)?;
        self.hit(CommitPoint::AfterPack)?;
        let new_index = DiskIndex::open(sealed.index.path())?;
        let index_id = io_util::install(sealed.index, &self.root.join("indexes"), ".idx")?;
        self.hit(CommitPoint::AfterIndex)?;
        let before = state.index.count;
        let before_packs = state.catalog.packs.len();
        let mut catalog = state.catalog.clone();
        catalog.generation = catalog
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("generation overflow"))?;
        catalog.index = Some(index_id);
        catalog.packs = sealed.pack.into_iter().collect();
        let after_packs = catalog.packs.len();
        self.publish(&mut state, catalog, new_index)?;
        // CURRENT is durably published before any old file is unlinked. On an
        // earlier failure both old and new blobs can safely remain on disk.
        self.cleanup_unreferenced(&state)?;
        Ok(GcReport {
            before_objects: before,
            live_objects: marks.count,
            removed_objects: before.saturating_sub(marks.count),
            packs_before: before_packs,
            packs_after: after_packs,
        })
    }
    fn cleanup_unreferenced(&self, state: &State) -> Result<()> {
        for (directory, suffix) in [
            ("packs", ".pack"),
            ("indexes", ".idx"),
            ("catalogs", ".cat"),
        ] {
            for entry in fs::read_dir(self.root.join(directory))? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(text) = name.strip_suffix(suffix) else {
                    continue;
                };
                let Ok(id) = text.parse::<Id>() else { continue };
                let keep = match directory {
                    "packs" => state.catalog.packs.contains(&id),
                    "indexes" => state.catalog.index == Some(id),
                    _ => state.digest == id,
                };
                if !keep {
                    fs::remove_file(entry.path())?;
                }
            }
            io_util::sync_dir(&self.root.join(directory))?;
        }
        Ok(())
    }
}
pub struct Snapshot<'a> {
    _guard: RwLockReadGuard<'a, ()>,
    pub(crate) repository: &'a Repository,
    store: DiskView,
    pub id: Id,
    pub revision: Revision,
    pub workspace: Workspace,
}
impl ReadStore for Snapshot<'_> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.store.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.store.get(id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        self.store.contains(id)
    }
}
impl Snapshot<'_> {
    pub fn stat(&self, path: &str) -> Result<(u64, Inode)> {
        namespace::stat(self, &self.workspace, path)
    }
    pub fn file_id(&self, path: &str) -> Result<Id> {
        namespace::file(self, &self.workspace, path)
    }
    pub fn file_id_follow(&self, path: &str) -> Result<Id> {
        let n = namespace::resolve_follow(self, &self.workspace, path)?;
        match namespace::inode(self, &self.workspace, n)?.data {
            InodeData::File(id) => Ok(id),
            _ => Err(invalid("not a regular file")),
        }
    }
    pub fn read_into(&self, path: &str, offset: u64, output: &mut [u8]) -> Result<usize> {
        pages::read_into(
            self,
            &self.repository.pages,
            self.file_id(path)?,
            offset,
            output,
        )
    }
    pub fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        pages::read_range(
            self,
            &self.repository.pages,
            self.file_id(path)?,
            offset,
            len,
        )
    }
    pub fn copy_to<W: Write>(&self, path: &str, writer: W) -> Result<u64> {
        pages::copy_to(self, &self.repository.pages, self.file_id(path)?, writer)
    }
    pub fn list<F: FnMut(&str, u64) -> Result<()>>(&self, path: &str, f: &mut F) -> Result<()> {
        namespace::list(self, &self.workspace, path, f)
    }
    pub fn reader(&self, path: &str) -> Result<pages::FileReader<'_, Self>> {
        pages::FileReader::new(self, &self.repository.pages, self.file_id(path)?)
    }
    pub fn read_link(&self, path: &str) -> Result<String> {
        namespace::read_link(self, &self.workspace, path)
    }
}
pub struct Transaction<'a> {
    _guard: RwLockReadGuard<'a, ()>,
    repository: &'a Repository,
    name: String,
    pub(crate) expected: Option<Id>,
    core: crate::transaction::Edit<OverlayStore>,
}
impl Transaction<'_> {
    pub fn commit(self, message: &str) -> Result<Id> {
        self.commit_with_parent(message, true)
    }
    fn commit_with_parent(self, message: &str, keep_parent: bool) -> Result<Id> {
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
