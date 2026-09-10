//! Bounded memory-first structures used by graph verification.

use super::*;
use crate::{
    model::{ChunkRef, Layout},
    store::{MemoryStore, Object, ObjectInfo, ReadStore, WriteStore, save},
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct CountingStore {
    inner: MemoryStore,
    chunk_gets: AtomicUsize,
}

impl ReadStore for CountingStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.inner.info(id)
    }

    fn get(&self, id: Id) -> Result<Object> {
        if self.inner.info(id)?.kind == Kind::Chunk {
            self.chunk_gets.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.get(id)
    }
}

impl WriteStore for CountingStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        self.inner.put(kind, raw)
    }
}

fn paged_file(store: &CountingStore, final_bytes: &[u8]) -> FileObject {
    let layout = Layout {
        page_size: 4096,
        min: 64,
        avg: 256,
        max: 1024,
    };
    let first = store.put(Kind::Chunk, b"first").unwrap();
    let final_id = store.put(Kind::Chunk, final_bytes).unwrap();
    let page = save(
        store,
        Kind::Page,
        &Page {
            page_size: layout.page_size,
            used: (5 + final_bytes.len()) as u32,
            extents: vec![
                PageExtent::Data(ChunkRef { id: first, len: 5 }),
                PageExtent::Data(ChunkRef {
                    id: final_id,
                    len: final_bytes.len() as u32,
                }),
            ],
        },
    )
    .unwrap();
    let pages = radix::set(store, None, &0u64.to_be_bytes(), Some(page)).unwrap();
    FileObject::paged(5 + final_bytes.len() as u64, layout, pages)
}

#[test]
fn verified_info_checks_lengths_without_reading_every_chunk() {
    let store = CountingStore::default();
    let file = paged_file(&store, b"last");

    verify_file_from_verified_info(&store, &file).unwrap();

    assert_eq!(store.chunk_gets.load(Ordering::Relaxed), 1);
}

#[test]
fn verified_info_still_rejects_explicit_trailing_zeroes() {
    let store = CountingStore::default();
    let file = paged_file(&store, b"last\0");

    assert!(verify_file_from_verified_info(&store, &file).is_err());
}

#[test]
fn marks_spill_without_changing_set_behavior() {
    let scratch = tempfile::tempdir().unwrap();
    let ids = [Id::sha256(b"one"), Id::sha256(b"two"), Id::sha256(b"three")];
    let mut marks = DiskMarks::with_memory_limit(scratch.path(), 2).unwrap();

    for id in ids {
        assert!(marks.insert(id).unwrap());
    }
    assert!(!marks.insert(ids[1]).unwrap());
    assert_eq!(marks.count, 3);
    for id in ids {
        assert!(marks.contains(id).unwrap());
    }

    let mut visited = HashSet::new();
    marks
        .visit(|id| {
            visited.insert(id);
            Ok(())
        })
        .unwrap();
    assert_eq!(visited, HashSet::from(ids));
}

#[test]
fn stack_spills_without_changing_lifo_order() {
    let scratch = tempfile::tempdir().unwrap();
    let mut stack = DiskStack::<1>::with_memory_limit(scratch.path(), 2).unwrap();

    stack.push([1]).unwrap();
    stack.push([2]).unwrap();
    stack.push([3]).unwrap();

    assert_eq!(stack.pop().unwrap(), Some([3]));
    assert_eq!(stack.pop().unwrap(), Some([2]));
    assert_eq!(stack.pop().unwrap(), Some([1]));
    assert_eq!(stack.pop().unwrap(), None);
}

#[test]
fn counters_spill_without_losing_values() {
    let scratch = tempfile::tempdir().unwrap();
    let mut counters = Counters::with_memory_limit(scratch.path(), 2).unwrap();

    counters.increment(1).unwrap();
    counters.increment(1).unwrap();
    counters.increment(2).unwrap();
    counters.increment(3).unwrap();
    counters.increment(3).unwrap();

    assert_eq!(counters.get(1).unwrap(), 2);
    assert_eq!(counters.get(2).unwrap(), 1);
    assert_eq!(counters.get(3).unwrap(), 2);
    assert_eq!(counters.get(4).unwrap(), 0);
}
