//! repeatable scale benchmark for streaming, multipart completion, metadata caching, and listing.

use super::*;

/// run with `cargo test -p runinator-blob-core fs_scale_benchmark -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "creates 100,000 objects and transfers hundreds of MiB"]
async fn fs_scale_benchmark() {
    let root = std::env::temp_dir().join(format!("runinator-blob-bench-{}", uuid::Uuid::now_v7()));
    let store = FsBlobStore::open(&root).await.unwrap();
    let bucket = "benchmark-bucket";
    store.create_bucket(bucket).await.unwrap();

    let stream_key = ObjectKey::parse("transfer/stream.bin").unwrap();
    let stream_size = 64 * 1024 * 1024u64;
    let mut source = tokio::io::repeat(0x42).take(stream_size);
    let put_started = std::time::Instant::now();
    store
        .put_stream(
            bucket,
            &stream_key,
            &mut source,
            Some(stream_size),
            PutOptions::default(),
        )
        .await
        .unwrap();
    let put_elapsed = put_started.elapsed();
    let get_started = std::time::Instant::now();
    let mut reader = store.open(bucket, &stream_key, None).await.unwrap();
    tokio::io::copy(&mut reader.body, &mut tokio::io::sink())
        .await
        .unwrap();
    let get_elapsed = get_started.elapsed();

    let multipart_key = ObjectKey::parse("transfer/multipart.bin").unwrap();
    let upload = store
        .create_multipart(bucket, &multipart_key, PutOptions::default())
        .await
        .unwrap();
    let mut completed = Vec::new();
    for number in 1..=16 {
        let part_size = 8 * 1024 * 1024u64;
        let mut part = tokio::io::repeat(number as u8).take(part_size);
        let etag = store
            .upload_part_stream(
                bucket,
                &multipart_key,
                &upload,
                number,
                &mut part,
                Some(part_size),
                PutOptions::default(),
            )
            .await
            .unwrap();
        completed.push(CompletedPart {
            part_number: number,
            etag,
        });
    }
    let complete_started = std::time::Instant::now();
    store
        .complete_multipart(bucket, &multipart_key, &upload, &completed)
        .await
        .unwrap();
    let complete_elapsed = complete_started.elapsed();

    for index in 0..100_000u32 {
        let key = ObjectKey::parse(&format!("listing/{:03}/{index:06}", index / 1000)).unwrap();
        store
            .put(bucket, &key, vec![index as u8], PutOptions::default())
            .await
            .unwrap();
    }
    let first_request = ListRequest {
        prefix: Some("listing/".into()),
        max_keys: Some(1000),
        ..Default::default()
    };
    let list_started = std::time::Instant::now();
    let (first, first_stats) = walk::page(
        bucket,
        &BucketPaths::new(&root, bucket),
        &first_request,
        &store.cache,
    )
    .unwrap();
    let first_list_ms = list_started.elapsed().as_millis();
    let second_request = ListRequest {
        prefix: Some("listing/".into()),
        continuation_token: first.next_continuation_token.clone(),
        max_keys: Some(1000),
        ..Default::default()
    };
    let next_started = std::time::Instant::now();
    let (second, second_stats) = walk::page(
        bucket,
        &BucketPaths::new(&root, bucket),
        &second_request,
        &store.cache,
    )
    .unwrap();
    let next_list_ms = next_started.elapsed().as_millis();

    let hot_key = ObjectKey::parse("listing/000/000000").unwrap();
    store.cache.remove(bucket, hot_key.as_str());
    let cold_started = std::time::Instant::now();
    store.head(bucket, &hot_key).await.unwrap();
    let cold_head_us = cold_started.elapsed().as_micros();
    let hot_started = std::time::Instant::now();
    store.head(bucket, &hot_key).await.unwrap();
    let hot_head_us = hot_started.elapsed().as_micros();

    println!(
        "stream_put_mib_s={:.1} stream_get_mib_s={:.1} multipart_complete_mib_s={:.1} \
         first_list_ms={first_list_ms} next_list_ms={next_list_ms} cold_head_us={cold_head_us} \
         hot_head_us={hot_head_us} first_count={} second_count={} first_visited={} \
         second_visited={} first_metadata_reads={} second_metadata_reads={}",
        throughput_mib(stream_size, put_elapsed),
        throughput_mib(stream_size, get_elapsed),
        throughput_mib(128 * 1024 * 1024, complete_elapsed),
        first.objects.len(),
        second.objects.len(),
        first_stats.visited_entries,
        second_stats.visited_entries,
        first_stats.metadata_reads,
        second_stats.metadata_reads,
    );
    assert_eq!(first.objects.len(), 1000);
    assert_eq!(second.objects.len(), 1000);
    assert_eq!(reader.meta.size, stream_size);
    drop(reader);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

fn throughput_mib(bytes: u64, elapsed: std::time::Duration) -> f64 {
    bytes as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
}
