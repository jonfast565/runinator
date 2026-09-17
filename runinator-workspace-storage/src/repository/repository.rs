#[allow(unused_imports)]
use super::*;

pub struct Repository {
    pub(crate) root: PathBuf,
    pub(super) _process_lock: File,
    // One process owns a repository. Within that process many threads may use
    // snapshots and optimistic transactions concurrently through Arc<Repository>.
    pub(super) gate: RwLock<()>,
    pub(super) state: Mutex<State>,
    pub(crate) config: Config,
    pub(crate) pages: ByteCache,
    pub(super) caches: Arc<ObjectCaches>,
    pub(super) tiny_blocks: Arc<ByteCache>,
    pub(super) failpoint: AtomicU8,
}

impl Repository {
    pub fn init(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        Self::open_impl(path.as_ref(), config, true)
    }
    pub fn open(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        Self::open_impl(path.as_ref(), config, false)
    }
    pub(super) fn open_impl(path: &Path, config: Config, create: bool) -> Result<Self> {
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
    pub(super) fn ensure(state: &State) -> Result<()> {
        if state.uncertain {
            return Err(Error::CommitUncertain("close and reopen repository".into()));
        }
        Ok(())
    }
    pub(super) fn raw(&self, state: &State) -> RawDisk {
        RawDisk {
            root: self.root.clone(),
            index: state.index.clone(),
            tiny_blocks: self.tiny_blocks.clone(),
        }
    }
    pub(super) fn view(&self, state: &State) -> DiskView {
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
    pub(super) fn hit(&self, point: CommitPoint) -> Result<()> {
        if self
            .failpoint
            .compare_exchange(point as u8, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Err(std::io::Error::other(format!("injected failure at {point:?}")).into());
        }
        Ok(())
    }
    pub(super) fn publish(
        &self,
        state: &mut State,
        catalog: Catalog,
        index: DiskIndex,
    ) -> Result<()> {
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
    pub(super) fn make_snapshot<'a>(
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
        catalog.packs.par_iter().try_for_each(|pack| {
            io_util::verify_file(&self.root.join("packs").join(format!("{pack}.pack")), *pack)?;
            Ok::<_, Error>(())
        })?;
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
    pub(super) fn cleanup_unreferenced(&self, state: &State) -> Result<()> {
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
