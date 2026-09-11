//! blob-store startup bucket initialization.

use std::sync::Arc;

use super::ensure_buckets;
use runinator_blob_core::{BlobStore, FsBlobStore};

#[tokio::test]
async fn ensure_buckets_creates_the_execution_profile_bucket() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FsBlobStore::open(root.path()).await.unwrap()) as Arc<dyn BlobStore>;

    ensure_buckets(&store).await.unwrap();

    assert!(store
        .bucket_exists(runinator_blob_core::EXECUTION_PROFILE_BUCKET)
        .await
        .unwrap());
}
