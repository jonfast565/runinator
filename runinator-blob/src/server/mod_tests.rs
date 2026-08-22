//! covers the http surface end to end: a real listener, a real signed client, and a real store.
//!
//! this is the test that would catch a signing drift between the two halves, which is precisely the
//! failure a hand-written signer invites — so it exercises the client against the server rather than
//! either against a fixture.

use super::*;
use crate::client::S3BlobClient;
use crate::config::BlobClientConfig;
use runinator_blob_core::sigv4::BlobCredential;
use runinator_blob_core::{ByteRange, ListRequest, ObjectKey, PutOptions};

const BUCKET: &str = "test-bucket";
const ACCESS_KEY_ID: &str = "AKIALOCALTESTKEY0000";
const SECRET: &str = "localtestsecretlocaltestsecretlocaltest0";

/// a running service plus a signed client pointed at it, and the temp directory to clean up.
struct Harness {
    client: S3BlobClient,
    root: std::path::PathBuf,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn harness(anonymous: bool) -> Harness {
    let root = std::env::temp_dir().join(format!("runinator-blob-http-{}", uuid::Uuid::now_v7()));
    let credential = BlobCredential {
        access_key_id: ACCESS_KEY_ID.into(),
        secret_access_key: SECRET.into(),
    };
    let credentials = if anonymous {
        runinator_blob_core::CredentialStore::default().allowing_anonymous()
    } else {
        runinator_blob_core::CredentialStore::new([credential.clone()])
    };
    let config = BlobServerConfig {
        // port 0 lets the os pick, so concurrent test binaries do not collide.
        listen_addr: "127.0.0.1:0".into(),
        data_dir: root.display().to_string(),
        region: runinator_blob_core::sigv4::DEFAULT_REGION.into(),
        credentials,
        max_object_bytes: 8 * 1024 * 1024,
    };

    let store = FsBlobStore::open(&root).await.unwrap();
    store.create_bucket(BUCKET).await.unwrap();
    let service = Arc::new(BlobService::new(Arc::new(store), config.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown, signal) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(service))
            .with_graceful_shutdown(async {
                let _ = signal.await;
            })
            .await;
    });

    let client = S3BlobClient::new(BlobClientConfig {
        endpoint: format!("http://{addr}/"),
        region: runinator_blob_core::sigv4::DEFAULT_REGION.into(),
        credential: (!anonymous).then_some(credential),
    })
    .unwrap();

    Harness {
        client,
        root,
        shutdown: Some(shutdown),
    }
}

fn key(raw: &str) -> ObjectKey {
    ObjectKey::parse(raw).unwrap()
}

#[tokio::test]
async fn round_trips_a_signed_object() {
    let harness = harness(false).await;
    let body = b"round trip through http".to_vec();
    harness
        .client
        .put(BUCKET, &key("a/b.bin"), body.clone(), PutOptions::default())
        .await
        .unwrap();

    let fetched = harness
        .client
        .get(BUCKET, &key("a/b.bin"), None)
        .await
        .unwrap();
    assert_eq!(fetched.data, body);
    let meta = harness.client.head(BUCKET, &key("a/b.bin")).await.unwrap();
    assert_eq!(meta.size, body.len() as u64);
    assert_eq!(meta.sha256, runinator_blob_core::sha256_hex(&body));
}

#[tokio::test]
async fn serves_a_ranged_read_over_http() {
    let harness = harness(false).await;
    harness
        .client
        .put(
            BUCKET,
            &key("data.bin"),
            (0u8..=255).collect(),
            PutOptions::default(),
        )
        .await
        .unwrap();
    let slice = harness
        .client
        .get(
            BUCKET,
            &key("data.bin"),
            Some(ByteRange::From {
                start: 10,
                end: Some(19),
            }),
        )
        .await
        .unwrap();
    assert_eq!(slice.data, (10u8..=19).collect::<Vec<_>>());
    // the object's true size survives a ranged read, which `content-length` alone would not report.
    assert_eq!(slice.meta.size, 256);
}

#[tokio::test]
async fn enforces_write_once_and_digest_verification() {
    let harness = harness(false).await;
    let body = b"immutable artifact".to_vec();
    let digest = runinator_blob_core::sha256_hex(&body);
    let options = PutOptions::content_addressed(digest);
    harness
        .client
        .put(BUCKET, &key("sha256/a.zip"), body.clone(), options.clone())
        .await
        .unwrap();
    assert!(matches!(
        harness
            .client
            .put(BUCKET, &key("sha256/a.zip"), body, options)
            .await,
        Err(runinator_blob_core::BlobError::AlreadyExists(_))
    ));

    let wrong = harness
        .client
        .put(
            BUCKET,
            &key("sha256/b.zip"),
            b"other".to_vec(),
            PutOptions::content_addressed("0".repeat(64)),
        )
        .await;
    assert!(matches!(
        wrong,
        Err(runinator_blob_core::BlobError::DigestMismatch { .. })
    ));
}

#[tokio::test]
async fn rejects_an_unsigned_request_when_credentials_are_required() {
    let harness = harness(false).await;
    let anonymous = S3BlobClient::new(BlobClientConfig {
        endpoint: harness_endpoint(&harness),
        region: runinator_blob_core::sigv4::DEFAULT_REGION.into(),
        credential: None,
    })
    .unwrap();
    let error = anonymous.head(BUCKET, &key("anything")).await.unwrap_err();
    assert!(matches!(
        error,
        runinator_blob_core::BlobError::Unauthorized(_)
    ));
}

#[tokio::test]
async fn rejects_a_wrong_secret() {
    let harness = harness(false).await;
    let impostor = S3BlobClient::new(BlobClientConfig {
        endpoint: harness_endpoint(&harness),
        region: runinator_blob_core::sigv4::DEFAULT_REGION.into(),
        credential: Some(BlobCredential {
            access_key_id: ACCESS_KEY_ID.into(),
            secret_access_key: "a-different-secret-entirely-0000000000000".into(),
        }),
    })
    .unwrap();
    assert!(matches!(
        impostor.head(BUCKET, &key("anything")).await,
        Err(runinator_blob_core::BlobError::Unauthorized(_))
    ));
}

#[tokio::test]
async fn allows_anonymous_when_configured() {
    let harness = harness(true).await;
    harness
        .client
        .put(
            BUCKET,
            &key("open.txt"),
            b"x".to_vec(),
            PutOptions::default(),
        )
        .await
        .unwrap();
    assert!(harness
        .client
        .exists(BUCKET, &key("open.txt"))
        .await
        .unwrap());
}

#[tokio::test]
async fn lists_buckets_and_objects_over_http() {
    let harness = harness(false).await;
    for name in ["a/one", "a/two", "b/three"] {
        harness
            .client
            .put(BUCKET, &key(name), b"x".to_vec(), PutOptions::default())
            .await
            .unwrap();
    }
    let listing = harness
        .client
        .list(BUCKET, &ListRequest::with_prefix("a/"))
        .await
        .unwrap();
    assert_eq!(
        listing
            .objects
            .iter()
            .map(|o| o.key.as_str())
            .collect::<Vec<_>>(),
        vec!["a/one", "a/two"]
    );

    let rolled = harness
        .client
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

    let buckets = harness.client.list_buckets().await.unwrap();
    assert!(buckets.iter().any(|bucket| bucket.name == BUCKET));
}

#[tokio::test]
async fn round_trips_a_multipart_upload_over_http() {
    let harness = harness(false).await;
    let target = key("multi.bin");
    let upload_id = harness
        .client
        .create_multipart(BUCKET, &target, PutOptions::default())
        .await
        .unwrap();
    let mut parts = Vec::new();
    for (index, chunk) in [vec![7u8; 16], vec![9u8; 16]].into_iter().enumerate() {
        let number = index as u32 + 1;
        let etag = harness
            .client
            .upload_part(BUCKET, &target, &upload_id, number, chunk)
            .await
            .unwrap();
        parts.push(runinator_blob_core::CompletedPart {
            part_number: number,
            etag,
        });
    }
    let meta = harness
        .client
        .complete_multipart(BUCKET, &target, &upload_id, &parts)
        .await
        .unwrap();
    assert_eq!(meta.size, 32);
    let fetched = harness.client.get(BUCKET, &target, None).await.unwrap();
    assert_eq!(&fetched.data[..16], &[7u8; 16]);
    assert_eq!(&fetched.data[16..], &[9u8; 16]);
}

#[tokio::test]
async fn reports_a_missing_object_as_not_found() {
    let harness = harness(false).await;
    assert!(matches!(
        harness.client.head(BUCKET, &key("absent")).await,
        Err(runinator_blob_core::BlobError::NotFound(_))
    ));
    assert!(matches!(
        harness.client.head("no-such-bucket", &key("absent")).await,
        Err(runinator_blob_core::BlobError::NoSuchBucket(_))
    ));
}

#[tokio::test]
async fn signs_keys_containing_characters_sdks_percent_encode() {
    let harness = harness(false).await;
    // `!`, `*`, `'`, `(`, `)` are legal in a key but are not sigv4 "unreserved", so they are
    // Percent-encode the URL path before signing. The signature must cover the encoded path, which is
    // exactly what this asserts.
    let awkward = key("weird/name!with*chars'(1).bin");
    harness
        .client
        .put(BUCKET, &awkward, b"ok".to_vec(), PutOptions::default())
        .await
        .unwrap();
    assert_eq!(
        harness
            .client
            .get(BUCKET, &awkward, None)
            .await
            .unwrap()
            .data,
        b"ok"
    );
}

fn harness_endpoint(harness: &Harness) -> String {
    // the client keeps the endpoint it was built with; rebuild it from the same place for a second
    // client pointed at the same service.
    harness.client.endpoint().to_string()
}
