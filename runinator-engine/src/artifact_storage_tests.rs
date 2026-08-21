//! covers both artifact storage shapes: `blob://` uris and the pre-blob local paths that still
//! have to be readable.

use super::*;
use runinator_blob_core::FsBlobStore;
use tokio::io::AsyncReadExt;

/// a store rooted in a fresh temporary directory, plus the directory guard that removes it.
struct Fixture {
    store: Arc<dyn BlobStore>,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("runinator-artifacts-{}", Uuid::now_v7()));
    let store = FsBlobStore::open(&root).await.unwrap();
    store.create_bucket(RUN_ARTIFACT_BUCKET).await.unwrap();
    Fixture {
        store: Arc::new(store),
        root,
    }
}

async fn read_all(mut content: ArtifactContent) -> Vec<u8> {
    let mut bytes = Vec::new();
    content.body.read_to_end(&mut bytes).await.unwrap();
    bytes
}

#[tokio::test]
async fn round_trips_through_the_object_store() {
    let fixture = fixture().await;
    let run_id = Uuid::now_v7();
    let uri = put_artifact(
        &fixture.store,
        run_id,
        "report.txt",
        "text/plain",
        b"artifact body",
    )
    .await
    .unwrap();

    // The key is run-scoped, so one run's artifacts list together.
    assert!(uri.starts_with(&format!("blob://{RUN_ARTIFACT_BUCKET}/runs/{run_id}/")));
    assert!(uri.ends_with("-report.txt"));

    let content = open_artifact(&fixture.store, &uri, None).await.unwrap();
    assert_eq!(content.size_bytes, 13);
    assert_eq!(read_all(content).await, b"artifact body");
}

#[tokio::test]
async fn reads_a_byte_range() {
    let fixture = fixture().await;
    let uri = put_artifact(
        &fixture.store,
        Uuid::now_v7(),
        "data.bin",
        "application/octet-stream",
        b"0123456789",
    )
    .await
    .unwrap();
    let content = open_artifact(
        &fixture.store,
        &uri,
        Some(ByteRange::From {
            start: 2,
            end: Some(5),
        }),
    )
    .await
    .unwrap();
    assert_eq!(read_all(content).await, b"2345");
}

#[tokio::test]
async fn a_hostile_filename_cannot_escape_its_run_prefix() {
    let fixture = fixture().await;
    let run_id = Uuid::now_v7();
    let uri = put_artifact(
        &fixture.store,
        run_id,
        "../../etc/passwd",
        "text/plain",
        b"x",
    )
    .await
    .unwrap();

    // the separators are gone, so the whole filename is one path segment under the run's prefix —
    // which is what makes it harmless. a literal `..` *inside* a segment is not traversal, so the
    // property worth asserting is the shape of the key, not the absence of dots.
    let (_, key) = runinator_blob_core::parse_blob_uri(&uri).unwrap();
    let expected_prefix = format!("runs/{run_id}/");
    let filename = key.as_str().strip_prefix(&expected_prefix).unwrap();
    assert!(
        !filename.contains('/'),
        "filename kept a separator: {filename}"
    );
    assert!(filename.ends_with("-.._.._etc_passwd"));
    assert!(open_artifact(&fixture.store, &uri, None).await.is_ok());
}

#[tokio::test]
async fn still_reads_a_pre_blob_local_path() {
    let fixture = fixture().await;
    let legacy = fixture.root.join("legacy-artifact.txt");
    tokio::fs::write(&legacy, b"written before the object store")
        .await
        .unwrap();
    let uri = legacy.display().to_string();

    let content = open_artifact(&fixture.store, &uri, None).await.unwrap();
    assert_eq!(content.size_bytes, 31);
    assert_eq!(read_all(content).await, b"written before the object store");

    // and deleting such a row still unlinks the file it points at.
    delete_artifact_bytes(&fixture.store, &uri).await;
    assert!(!legacy.exists());
}

#[tokio::test]
async fn deleting_is_idempotent_in_both_shapes() {
    let fixture = fixture().await;
    let uri = put_artifact(
        &fixture.store,
        Uuid::now_v7(),
        "gone.txt",
        "text/plain",
        b"x",
    )
    .await
    .unwrap();
    delete_artifact_bytes(&fixture.store, &uri).await;
    // a second delete must not panic or block the row delete that follows it.
    delete_artifact_bytes(&fixture.store, &uri).await;
    delete_artifact_bytes(&fixture.store, "/no/such/path").await;
    assert!(open_artifact(&fixture.store, &uri, None).await.is_err());
}
