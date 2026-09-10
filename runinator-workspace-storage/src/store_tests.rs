//! Ordered parallel batch reads.

use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct ConcurrentStore {
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl ReadStore for ConcurrentStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        Ok(ObjectInfo {
            kind: Kind::Chunk,
            raw_len: id.0[0] as usize,
        })
    }

    fn get(&self, id: Id) -> Result<Object> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(Object {
            kind: Kind::Chunk,
            bytes: Arc::new(vec![id.0[0]]),
        })
    }
}

#[test]
fn default_batch_reads_concurrently_and_preserves_order() -> Result<()> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let store = ConcurrentStore {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    };
    let ids = (1..=16)
        .map(|value| {
            let mut bytes = [0; 32];
            bytes[0] = value;
            Id(bytes)
        })
        .collect::<Vec<_>>();

    let objects = pool.install(|| store.get_many(&ids))?;

    assert!(store.peak.load(Ordering::SeqCst) > 1);
    assert_eq!(
        objects
            .iter()
            .map(|object| object.bytes[0])
            .collect::<Vec<_>>(),
        (1..=16).collect::<Vec<_>>()
    );
    Ok(())
}
