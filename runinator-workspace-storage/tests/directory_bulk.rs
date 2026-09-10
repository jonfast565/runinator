//! Ordered directory metadata batches and cache reuse.
use runinator_workspace_storage::{
    Config, Id, Repository, Result,
    cache::BufferedStore,
    model::Kind,
    store::{Object, ObjectInfo, ReadStore},
    view::View,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Counted<S> {
    inner: S,
    reads: AtomicUsize,
    batches: Mutex<Vec<usize>>,
}
impl<S: ReadStore> ReadStore for Counted<S> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.get(id)
    }
    fn get_many(&self, ids: &[Id]) -> Result<Vec<Object>> {
        self.batches.lock().unwrap().push(ids.len());
        ids.iter().map(|id| self.get(*id)).collect()
    }
}
#[test]
fn directory_pages_are_ordered_complete_and_reuse_cached_metadata() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let repo = Repository::init(temp.path(), Config::default())?;
    let mut tx = repo.transaction("main")?;
    for n in 0..201 {
        tx.put(&format!("file-{n:04}"), format!("payload-{n}").as_bytes())?;
    }
    let id = tx.commit("directory fixture")?;
    let snapshot = repo.snapshot("main")?;
    let store = BufferedStore::new(
        Counted {
            inner: snapshot,
            reads: AtomicUsize::new(0),
            batches: Mutex::new(Vec::new()),
        },
        16 * 1024 * 1024,
    );
    let view = View::new(store, id)?;
    let first = view.directory("", None, 200)?;
    assert_eq!(first.len(), 200);
    assert_eq!(first[0].name, "file-0000");
    let second = view.directory("", Some(&first[199].name), 200)?;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name, "file-0200");
    assert_eq!(second[0].size, 11);
    let count = view.store.inner.reads.load(Ordering::Relaxed);
    view.directory("", None, 200)?;
    assert_eq!(view.store.inner.reads.load(Ordering::Relaxed), count);
    assert!(
        view.store
            .inner
            .batches
            .lock()
            .unwrap()
            .iter()
            .any(|n| *n >= 200)
    );
    Ok(())
}
#[test]
fn batch_preserves_duplicates_and_missing_object_errors() -> Result<()> {
    use runinator_workspace_storage::store::{MemoryStore, WriteStore};
    let memory = MemoryStore::default();
    let a = memory.put(Kind::Chunk, b"a")?;
    let b = memory.put(Kind::Chunk, b"b")?;
    let store = BufferedStore::new(
        Counted {
            inner: memory,
            reads: AtomicUsize::new(0),
            batches: Mutex::new(Vec::new()),
        },
        4096,
    );
    let batch = store.get_many(&[b, a, b])?;
    assert_eq!(&*batch[0].bytes, b"b");
    assert_eq!(&*batch[1].bytes, b"a");
    assert_eq!(&*batch[2].bytes, b"b");
    assert_eq!(store.inner.reads.load(Ordering::Relaxed), 2);
    assert!(store.get_many(&[Id::sha256(b"missing")]).is_err());
    Ok(())
}

#[test]
fn shared_cache_bulk_reads_retain_objects_and_validate_types() -> Result<()> {
    use runinator_workspace_storage::{
        cache::{CachedStore, ObjectCaches},
        model::FileObject,
        store::{MemoryStore, WriteStore, load_many},
    };
    let memory = MemoryStore::default();
    let id = memory.put(Kind::Chunk, b"not a file")?;
    let store = CachedStore {
        inner: Counted {
            inner: memory,
            reads: AtomicUsize::new(0),
            batches: Mutex::new(Vec::new()),
        },
        caches: std::sync::Arc::new(ObjectCaches::new(4096, 4096)),
    };
    assert_eq!(store.get_many(&[id, id])?.len(), 2);
    assert_eq!(store.inner.reads.load(Ordering::Relaxed), 1);
    store.get_many(&[id])?;
    assert_eq!(store.inner.reads.load(Ordering::Relaxed), 1);
    assert!(load_many::<FileObject, _>(&store, &[id], Kind::File).is_err());
    Ok(())
}
