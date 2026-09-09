//! Cancellation of validation that is still running on a blocking worker.
use super::*;
use storage::{
    model::Kind,
    store::{MemoryStore, WriteStore},
};

#[tokio::test]
async fn dropping_request_stops_subsequent_validation_reads() {
    let source = MemoryStore::default();
    let id = source.put(Kind::Chunk, b"repository data").unwrap();
    let guard = ValidationGuard::new();
    let store = guard.store(source);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (continue_tx, continue_rx) = std::sync::mpsc::channel();
    let validation = tokio::task::spawn_blocking(move || {
        assert!(store.get(id).is_ok());
        ready_tx.send(()).unwrap();
        continue_rx.recv().unwrap();
        assert!(matches!(store.get(id), Err(storage::Error::Conflict)));
        assert!(matches!(store.info(id), Err(storage::Error::Conflict)));
    });
    ready_rx.await.unwrap();
    drop(guard);
    continue_tx.send(()).unwrap();
    validation.await.unwrap();
}
