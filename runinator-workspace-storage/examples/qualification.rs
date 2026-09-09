//! Streaming scale qualification: 100 GiB of incompressible input and 100,000 entries by default.
use runinator_workspace_storage::{
    self as storage, Id,
    staging::{EmptyStore, Staging},
    store::{Object, ObjectInfo, ReadStore},
    transaction::Edit,
    view::View,
};
use std::{
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
};
struct Random {
    remaining: u64,
    state: u64,
}
impl Read for Random {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let n = self.remaining.min(output.len() as u64) as usize;
        for chunk in output[..n].chunks_mut(8) {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            chunk.copy_from_slice(&self.state.to_le_bytes()[..chunk.len()]);
        }
        self.remaining -= n as u64;
        Ok(n)
    }
}
struct Count<S> {
    inner: S,
    bytes: AtomicU64,
}
impl<S: ReadStore> ReadStore for Count<S> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        self.inner.info(id)
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        let object = self.inner.get(id)?;
        self.bytes
            .fetch_add(object.bytes.len() as u64, Ordering::Relaxed);
        Ok(object)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gib: u64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "100".into())
        .parse()?;
    let entries: u64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "100000".into())
        .parse()?;
    let scratch = tempfile::tempdir()?;
    let start = std::time::Instant::now();
    let mut edit = Edit::new(
        Staging::new(EmptyStore, scratch.path())?,
        None,
        storage::Layout::default(),
        16 * 1024 * 1024,
    )?;
    let length = gib * 1024 * 1024 * 1024;
    edit.put_sized(
        "large.bin",
        Random {
            remaining: length,
            state: 0x124a_fcdc_9876_1234,
        },
        length,
    )?;
    eprintln!("ingested {gib} GiB in {:?}", start.elapsed());
    for n in 1..entries {
        edit.put(
            &format!("entry-{n:06}"),
            Random {
                remaining: 128,
                state: n + 1,
            },
        )?;
        if n % 10000 == 0 {
            eprintln!("captured {n} entries in {:?}", start.elapsed());
        }
    }
    let revision = edit.finish("qualification", None)?;
    let count = Count {
        inner: &edit.store,
        bytes: AtomicU64::new(0),
    };
    let view = View::new(&count, revision)?;
    let range = view.read_range("large.bin", length / 2 + 123, 1024 * 1024)?;
    assert_eq!(range.len(), 1024 * 1024);
    let read_bytes = count.bytes.load(Ordering::Relaxed);
    assert!(
        read_bytes < 64 * 1024 * 1024,
        "range fetched {read_bytes} bytes"
    );
    let mut after = None;
    let mut listed = 0;
    loop {
        let page = view.directory("", after.as_deref(), 200)?;
        if page.is_empty() {
            break;
        }
        listed += page.len() as u64;
        after = page.last().map(|entry| entry.name.clone());
    }
    assert_eq!(listed, entries);
    let mut packs = 0u64;
    let mut packed_bytes = 0u64;
    storage::packs::seal(&edit.store, &EmptyStore, revision, scratch.path(), |pack| {
        let bytes = std::fs::metadata(pack.path)?.len();
        assert!(bytes <= 80 * 1024 * 1024);
        packs += 1;
        packed_bytes += bytes;
        if packs.is_multiple_of(100) {
            eprintln!("sealed {packs} packs in {:?}", start.elapsed());
        }
        Ok(())
    })?;
    println!(
        "revision={revision} logical_bytes={length} entries={entries} range_storage_bytes={read_bytes} packs={packs} packed_bytes={packed_bytes} elapsed_seconds={}",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
