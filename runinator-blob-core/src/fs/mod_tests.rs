//! covers the filesystem backend's object lifecycle, ranged reads, write-once, and listing.

use super::*;
use crate::listing::ListRequest;

const BUCKET: &str = "test-bucket";

/// a store rooted in a fresh temporary directory, plus the directory guard that removes it.
struct Fixture {
    store: FsBlobStore,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("runinator-blob-{}", uuid::Uuid::now_v7()));
    let store = FsBlobStore::open(&root).await.unwrap();
    store.create_bucket(BUCKET).await.unwrap();
    Fixture { store, root }
}

fn key(raw: &str) -> ObjectKey {
    ObjectKey::parse(raw).unwrap()
}

#[tokio::test]
async fn round_trips_an_object() {
    let fixture = fixture().await;
    let body = b"hello blob store".to_vec();
    let meta = fixture
        .store
        .put(BUCKET, &key("a/b.txt"), body.clone(), PutOptions::default())
        .await
        .unwrap();
    assert_eq!(meta.size, body.len() as u64);
    assert_eq!(meta.sha256, crate::meta::sha256_hex(&body));
    assert_eq!(meta.content_type, DEFAULT_CONTENT_TYPE);

    let fetched = fixture
        .store
        .get(BUCKET, &key("a/b.txt"), None)
        .await
        .unwrap();
    assert_eq!(fetched.data, body);
    assert_eq!(
        fixture
            .store
            .head(BUCKET, &key("a/b.txt"))
            .await
            .unwrap()
            .sha256,
        meta.sha256
    );
    assert!(fixture.store.exists(BUCKET, &key("a/b.txt")).await.unwrap());
}

#[tokio::test]
async fn reads_a_byte_range() {
    let fixture = fixture().await;
    fixture
        .store
        .put(
            BUCKET,
            &key("data.bin"),
            (0u8..=255).collect(),
            PutOptions::default(),
        )
        .await
        .unwrap();

    let slice = fixture
        .store
        .get(
            BUCKET,
            &key("data.bin"),
            Some(ByteRange::From {
                start: 16,
                end: Some(31),
            }),
        )
        .await
        .unwrap();
    assert_eq!(slice.data, (16u8..=31).collect::<Vec<_>>());
    let range = slice.range.unwrap();
    assert_eq!(range.content_range(), "bytes 16-31/256");

    let tail = fixture
        .store
        .get(BUCKET, &key("data.bin"), Some(ByteRange::Suffix(4)))
        .await
        .unwrap();
    assert_eq!(tail.data, vec![252, 253, 254, 255]);
}

#[tokio::test]
async fn write_once_refuses_a_second_write() {
    let fixture = fixture().await;
    let body = b"immutable".to_vec();
    let digest = crate::meta::sha256_hex(&body);
    let options = PutOptions::content_addressed(digest.clone());
    fixture
        .store
        .put(
            BUCKET,
            &key("sha256/artifact.zip"),
            body.clone(),
            options.clone(),
        )
        .await
        .unwrap();

    let second = fixture
        .store
        .put(BUCKET, &key("sha256/artifact.zip"), body, options)
        .await;
    assert!(matches!(second, Err(BlobError::AlreadyExists(_))));
}

#[tokio::test]
async fn verifies_the_expected_digest_before_writing() {
    let fixture = fixture().await;
    let error = fixture
        .store
        .put(
            BUCKET,
            &key("bad.bin"),
            b"actual bytes".to_vec(),
            PutOptions::content_addressed("0".repeat(64)),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, BlobError::DigestMismatch { .. }));
    // a rejected write leaves nothing behind, so a retry is not fighting a partial object.
    assert!(!fixture.store.exists(BUCKET, &key("bad.bin")).await.unwrap());
}

#[tokio::test]
async fn deletes_idempotently_and_reports_missing_reads() {
    let fixture = fixture().await;
    fixture
        .store
        .put(
            BUCKET,
            &key("gone.txt"),
            b"x".to_vec(),
            PutOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .store
        .delete(BUCKET, &key("gone.txt"))
        .await
        .unwrap();
    // a second delete is a success; s3 delete is idempotent.
    fixture
        .store
        .delete(BUCKET, &key("gone.txt"))
        .await
        .unwrap();
    assert!(matches!(
        fixture.store.head(BUCKET, &key("gone.txt")).await,
        Err(BlobError::NotFound(_))
    ));
}

#[tokio::test]
async fn lists_with_prefix_delimiter_and_paging() {
    let fixture = fixture().await;
    for name in ["a/one.txt", "a/two.txt", "b/three.txt", "top.txt"] {
        fixture
            .store
            .put(BUCKET, &key(name), b"x".to_vec(), PutOptions::default())
            .await
            .unwrap();
    }

    let all = fixture
        .store
        .list(BUCKET, &ListRequest::default())
        .await
        .unwrap();
    assert_eq!(all.objects.len(), 4);
    assert!(!all.is_truncated);

    let prefixed = fixture
        .store
        .list(BUCKET, &ListRequest::with_prefix("a/"))
        .await
        .unwrap();
    assert_eq!(
        prefixed
            .objects
            .iter()
            .map(|object| object.key.as_str())
            .collect::<Vec<_>>(),
        vec!["a/one.txt", "a/two.txt"]
    );

    let rolled = fixture
        .store
        .list(
            BUCKET,
            &ListRequest {
                delimiter: Some("/".into()),
                ..ListRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        rolled.common_prefixes,
        vec!["a/".to_string(), "b/".to_string()]
    );
    assert_eq!(
        rolled
            .objects
            .iter()
            .map(|object| object.key.as_str())
            .collect::<Vec<_>>(),
        vec!["top.txt"]
    );

    let first_page = fixture
        .store
        .list(
            BUCKET,
            &ListRequest {
                max_keys: Some(2),
                ..ListRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(first_page.is_truncated);
    let second_page = fixture
        .store
        .list(
            BUCKET,
            &ListRequest {
                max_keys: Some(2),
                continuation_token: first_page.next_continuation_token.clone(),
                ..ListRequest::default()
            },
        )
        .await
        .unwrap();
    let paged: Vec<&str> = first_page
        .objects
        .iter()
        .chain(second_page.objects.iter())
        .map(|object| object.key.as_str())
        .collect();
    assert_eq!(
        paged,
        vec!["a/one.txt", "a/two.txt", "b/three.txt", "top.txt"]
    );
}

#[tokio::test]
async fn assembles_a_multipart_upload() {
    let fixture = fixture().await;
    let target = key("big.bin");
    let upload_id = fixture
        .store
        .create_multipart(BUCKET, &target, PutOptions::default())
        .await
        .unwrap();

    let parts_data = [vec![1u8; 8], vec![2u8; 8]];
    let mut parts = Vec::new();
    for (index, data) in parts_data.iter().enumerate() {
        let number = index as u32 + 1;
        let etag = fixture
            .store
            .upload_part(BUCKET, &target, &upload_id, number, data.clone())
            .await
            .unwrap();
        parts.push(CompletedPart {
            part_number: number,
            etag,
        });
    }

    let meta = fixture
        .store
        .complete_multipart(BUCKET, &target, &upload_id, &parts)
        .await
        .unwrap();
    assert_eq!(meta.size, 16);
    let fetched = fixture.store.get(BUCKET, &target, None).await.unwrap();
    assert_eq!(
        fetched.data,
        [parts_data[0].clone(), parts_data[1].clone()].concat()
    );
    // completion clears the staging directory, so the upload id no longer resolves.
    assert!(fixture
        .store
        .upload_part(BUCKET, &target, &upload_id, 1, vec![0])
        .await
        .is_err());
}

#[tokio::test]
async fn refuses_unknown_buckets_and_non_empty_bucket_deletes() {
    let fixture = fixture().await;
    assert!(matches!(
        fixture.store.head("other-bucket", &key("x")).await,
        Err(BlobError::NoSuchBucket(_))
    ));
    fixture
        .store
        .put(BUCKET, &key("x"), b"x".to_vec(), PutOptions::default())
        .await
        .unwrap();
    assert!(matches!(
        fixture.store.delete_bucket(BUCKET).await,
        Err(BlobError::BucketNotEmpty(_))
    ));
    fixture.store.delete(BUCKET, &key("x")).await.unwrap();
    fixture.store.delete_bucket(BUCKET).await.unwrap();
    assert!(!fixture.store.bucket_exists(BUCKET).await.unwrap());
}
