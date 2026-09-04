//! execution-profile publication object-store setup.

use super::{EXECUTION_PROFILE_BUCKET, ensure_execution_profile_bucket};
use runinator_blob_core::{BlobStore, FsBlobStore};

#[tokio::test]
async fn publish_initialization_creates_the_missing_bucket() {
    let root = std::env::temp_dir().join(format!(
        "runinator-execution-profile-bucket-test-{}",
        uuid::Uuid::new_v4()
    ));
    let store = FsBlobStore::open(&root).await.unwrap();

    assert!(!store.bucket_exists(EXECUTION_PROFILE_BUCKET).await.unwrap());
    ensure_execution_profile_bucket(&store).await.unwrap();
    assert!(store.bucket_exists(EXECUTION_PROFILE_BUCKET).await.unwrap());

    std::fs::remove_dir_all(root).unwrap();
}
