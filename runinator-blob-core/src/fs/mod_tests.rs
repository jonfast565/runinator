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
async fn streams_an_object_without_exposing_the_footer() {
    let fixture = fixture().await;
    let target = key("stream.bin");
    let size = 8 * 1024 * 1024u64;
    let mut source = tokio::io::repeat(0x5a).take(size);
    let meta = fixture
        .store
        .put_stream(
            BUCKET,
            &target,
            &mut source,
            Some(size),
            PutOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(meta.size, size);
    let stored = std::fs::metadata(
        fixture
            .root
            .join(BUCKET)
            .join("objects")
            .join("stream.bin.blob"),
    )
    .unwrap();
    assert!(stored.len() > size);
    let fetched = fixture.store.get(BUCKET, &target, None).await.unwrap();
    assert_eq!(fetched.data.len() as u64, size);
    assert!(fetched.data.iter().all(|byte| *byte == 0x5a));
}

#[tokio::test]
async fn permits_an_object_and_a_child_key() {
    let fixture = fixture().await;
    fixture
        .store
        .put(BUCKET, &key("a"), b"parent".to_vec(), PutOptions::default())
        .await
        .unwrap();
    fixture
        .store
        .put(
            BUCKET,
            &key("a/b"),
            b"child".to_vec(),
            PutOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .get(BUCKET, &key("a"), None)
            .await
            .unwrap()
            .data,
        b"parent"
    );
    assert_eq!(
        fixture
            .store
            .get(BUCKET, &key("a/b"), None)
            .await
            .unwrap()
            .data,
        b"child"
    );
    let listed = fixture
        .store
        .list(BUCKET, &ListRequest::default())
        .await
        .unwrap();
    assert_eq!(
        listed
            .objects
            .iter()
            .map(|object| object.key.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "a/b"]
    );
}

#[tokio::test]
async fn replacement_updates_cached_metadata_and_concurrent_writes_stay_consistent() {
    let fixture = fixture().await;
    let target = key("replace.bin");
    fixture
        .store
        .put(BUCKET, &target, b"first".to_vec(), PutOptions::default())
        .await
        .unwrap();
    assert_eq!(fixture.store.head(BUCKET, &target).await.unwrap().size, 5);
    fixture
        .store
        .put(
            BUCKET,
            &target,
            b"second-value".to_vec(),
            PutOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.store.head(BUCKET, &target).await.unwrap().size, 12);

    let writes = tokio::join!(
        fixture
            .store
            .put(BUCKET, &target, vec![1; 1], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![2; 2], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![3; 3], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![4; 4], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![5; 5], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![6; 6], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![7; 7], PutOptions::default()),
        fixture
            .store
            .put(BUCKET, &target, vec![8; 8], PutOptions::default()),
    );
    for write in [
        writes.0, writes.1, writes.2, writes.3, writes.4, writes.5, writes.6, writes.7,
    ] {
        write.unwrap();
    }
    let fetched = fixture.store.get(BUCKET, &target, None).await.unwrap();
    assert_eq!(fetched.meta.size, fetched.data.len() as u64);
    assert_eq!(fetched.meta.sha256, crate::sha256_hex(&fetched.data));
}

#[tokio::test]
async fn evicts_metadata_by_weight_without_affecting_reads() {
    let root = std::env::temp_dir().join(format!("runinator-blob-cache-{}", uuid::Uuid::now_v7()));
    let store = FsBlobStore::open_with_options(
        &root,
        FsBlobStoreOptions {
            metadata_cache_bytes: 16 * 512,
            ..FsBlobStoreOptions::default()
        },
    )
    .await
    .unwrap();
    store.create_bucket(BUCKET).await.unwrap();
    for index in 0..100 {
        store
            .put(
                BUCKET,
                &key(&format!("cache/{index:03}")),
                vec![index as u8],
                PutOptions::default(),
            )
            .await
            .unwrap();
    }
    let evicted = (0..100)
        .map(|index| format!("cache/{index:03}"))
        .find(|candidate| store.cache.get(BUCKET, candidate).is_none())
        .expect("the deliberately small cache should evict an entry");
    assert_eq!(store.head(BUCKET, &key(&evicted)).await.unwrap().size, 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
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
    // A second delete succeeds because S3 delete is idempotent.
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

    let mut rejected = std::io::Cursor::new(b"rejected part".to_vec());
    assert!(matches!(
        fixture
            .store
            .upload_part_stream(
                BUCKET,
                &target,
                &upload_id,
                1,
                &mut rejected,
                Some(13),
                PutOptions::content_addressed("0".repeat(64)),
            )
            .await,
        Err(BlobError::DigestMismatch { .. })
    ));

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

#[tokio::test]
async fn migrates_a_legacy_bucket_before_opening() {
    let root = std::env::temp_dir().join(format!("runinator-blob-v1-{}", uuid::Uuid::now_v7()));
    let bucket_root = root.join(BUCKET);
    let data = b"legacy object".to_vec();
    let meta = ObjectMeta {
        key: "old/path.bin".into(),
        size: data.len() as u64,
        sha256: crate::sha256_hex(&data),
        content_type: "application/octet-stream".into(),
        last_modified: Utc::now(),
        metadata: BTreeMap::new(),
    };
    std::fs::create_dir_all(bucket_root.join("data/old")).unwrap();
    std::fs::create_dir_all(bucket_root.join("meta/old")).unwrap();
    std::fs::create_dir_all(bucket_root.join("uploads")).unwrap();
    std::fs::create_dir_all(bucket_root.join(".tmp")).unwrap();
    std::fs::write(bucket_root.join(".bucket"), b"").unwrap();
    std::fs::write(bucket_root.join("data/old/path.bin"), &data).unwrap();
    std::fs::write(
        bucket_root.join("meta/old/path.bin.json"),
        serde_json::to_vec(&meta).unwrap(),
    )
    .unwrap();

    let store = FsBlobStore::open(&root).await.unwrap();
    assert_eq!(
        store
            .get(BUCKET, &key("old/path.bin"), None)
            .await
            .unwrap()
            .data,
        data
    );
    assert!(bucket_root.join("objects/old/path.bin.blob").exists());
    assert!(!bucket_root.join("data").exists());
    assert!(!bucket_root.join("meta").exists());
    assert_eq!(
        std::fs::read(bucket_root.join(".bucket")).unwrap(),
        paths::V2_MARKER
    );
    drop(store);

    // a crash can leave both the committed v2 object and its legacy pair. reopening validates the
    // committed object before deleting the duplicate sources and marking the bucket complete.
    std::fs::create_dir_all(bucket_root.join("data/old")).unwrap();
    std::fs::create_dir_all(bucket_root.join("meta/old")).unwrap();
    std::fs::write(bucket_root.join(".bucket"), b"").unwrap();
    std::fs::write(bucket_root.join("data/old/path.bin"), &data).unwrap();
    std::fs::write(
        bucket_root.join("meta/old/path.bin.json"),
        serde_json::to_vec(&meta).unwrap(),
    )
    .unwrap();
    let resumed = FsBlobStore::open(&root).await.unwrap();
    assert!(!bucket_root.join("data").exists());
    assert!(!bucket_root.join("meta").exists());
    assert_eq!(
        resumed
            .get(BUCKET, &key("old/path.bin"), None)
            .await
            .unwrap()
            .data,
        data
    );
    drop(resumed);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_corrupt_legacy_object_blocks_migration_without_deleting_it() {
    let root = std::env::temp_dir().join(format!("runinator-blob-v1-bad-{}", uuid::Uuid::now_v7()));
    let bucket_root = root.join(BUCKET);
    let meta = ObjectMeta {
        key: "bad.bin".into(),
        size: 4,
        sha256: "0".repeat(64),
        content_type: DEFAULT_CONTENT_TYPE.into(),
        last_modified: Utc::now(),
        metadata: BTreeMap::new(),
    };
    std::fs::create_dir_all(bucket_root.join("data")).unwrap();
    std::fs::create_dir_all(bucket_root.join("meta")).unwrap();
    std::fs::write(bucket_root.join(".bucket"), b"").unwrap();
    std::fs::write(bucket_root.join("data/bad.bin"), b"data").unwrap();
    std::fs::write(
        bucket_root.join("meta/bad.bin.json"),
        serde_json::to_vec(&meta).unwrap(),
    )
    .unwrap();

    assert!(matches!(
        FsBlobStore::open(&root).await,
        Err(BlobError::DigestMismatch { .. })
    ));
    assert!(bucket_root.join("data/bad.bin").exists());
    assert!(bucket_root.join("meta/bad.bin.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn migration_removes_abandoned_temporary_files_before_cutover() {
    let root = std::env::temp_dir().join(format!("runinator-blob-v1-tmp-{}", uuid::Uuid::now_v7()));
    let bucket_root = root.join(BUCKET);
    std::fs::create_dir_all(bucket_root.join(".tmp")).unwrap();
    std::fs::write(bucket_root.join(".bucket"), b"").unwrap();
    std::fs::write(bucket_root.join(".tmp/abandoned"), b"partial").unwrap();

    let store = FsBlobStore::open(&root).await.unwrap();
    assert!(!bucket_root.join(".tmp/abandoned").exists());
    assert_eq!(
        std::fs::read(bucket_root.join(".bucket")).unwrap(),
        paths::V2_MARKER
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
