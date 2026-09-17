#[allow(unused_imports)]
use super::*;

pub struct FsBlobStore {
    pub(super) root: PathBuf,
    pub(super) options: FsBlobStoreOptions,
    pub(super) cache: Arc<MetadataCache>,
    pub(super) buckets: RwLock<HashMap<String, DateTime<Utc>>>,
    pub(super) key_locks: Vec<Mutex<()>>,
    pub(super) mutations: Semaphore,
}

impl FsBlobStore {
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_options(root, FsBlobStoreOptions::default()).await
    }

    pub async fn open_with_options(
        root: impl Into<PathBuf>,
        options: FsBlobStoreOptions,
    ) -> Result<Self> {
        if options.io_buffer_bytes == 0 || options.max_concurrent_writes == 0 {
            return Err(BlobError::BadRequest(
                "blob io buffer and write concurrency must be positive".into(),
            ));
        }
        let root = root.into();
        fs::create_dir_all(&root)
            .await
            .map_err(|err| BlobError::Io(format!("creating {}: {err}", root.display())))?;
        migrate::migrate_root(&root, options.io_buffer_bytes).await?;
        let buckets = load_buckets(&root).await?;
        Ok(Self {
            root,
            cache: Arc::new(MetadataCache::new(options.metadata_cache_bytes)),
            buckets: RwLock::new(buckets),
            key_locks: (0..KEY_LOCK_SHARDS).map(|_| Mutex::new(())).collect(),
            mutations: Semaphore::new(options.max_concurrent_writes),
            options,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(super) async fn bucket(&self, bucket: &str) -> Result<BucketPaths> {
        validate_bucket(bucket)?;
        if !self
            .buckets
            .read()
            .map_err(|_| BlobError::Io("bucket cache lock poisoned".into()))?
            .contains_key(bucket)
        {
            return Err(BlobError::NoSuchBucket(bucket.to_string()));
        }
        Ok(BucketPaths::new(&self.root, bucket))
    }

    pub(super) fn key_lock(&self, bucket: &str, key: &ObjectKey) -> &Mutex<()> {
        let mut hasher = DefaultHasher::new();
        bucket.hash(&mut hasher);
        key.as_str().hash(&mut hasher);
        &self.key_locks[hasher.finish() as usize % self.key_locks.len()]
    }

    pub(super) async fn read_meta(
        &self,
        bucket: &str,
        paths: &BucketPaths,
        key: &ObjectKey,
    ) -> Result<(ObjectMeta, bool)> {
        if let Some(meta) = self.cache.get(bucket, key.as_str()) {
            return Ok(((*meta).clone(), true));
        }
        let path = paths.object(key);
        let mut file = fs::File::open(&path)
            .await
            .map_err(|err| paths::read_error(&path, key.as_str(), err))?;
        let (meta, encoded_len) = format::read_object(&mut file, &path).await?;
        if meta.key != key.as_str() {
            return Err(BlobError::Io(format!(
                "blob metadata key '{}' disagrees with path '{key}'",
                meta.key
            )));
        }
        self.cache.insert(bucket, meta.clone(), encoded_len);
        Ok((meta, false))
    }

    pub(super) async fn write_object_stream(
        &self,
        bucket: &str,
        paths: &BucketPaths,
        key: &ObjectKey,
        body: &mut (dyn AsyncRead + Send + Unpin),
        content_length: Option<u64>,
        options: PutOptions,
    ) -> Result<ObjectMeta> {
        let started = std::time::Instant::now();
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let _key_guard = self.key_lock(bucket, key).lock().await;
        let (staged, mut file) =
            paths::create_staged(paths, &format!("object-{}", uuid::Uuid::now_v7())).await?;
        let result = async {
            let (size, sha256) =
                migrate::copy_hashed(body, &mut file, self.options.io_buffer_bytes).await?;
            if content_length.is_some_and(|expected| expected != size) {
                return Err(BlobError::BadRequest(format!(
                    "content length declared {} bytes but received {size}",
                    content_length.unwrap_or_default()
                )));
            }
            if let Some(expected) = &options.expected_sha256 {
                if !expected.eq_ignore_ascii_case(&sha256) {
                    return Err(BlobError::DigestMismatch {
                        expected: expected.clone(),
                        actual: sha256,
                    });
                }
            }
            let meta = ObjectMeta {
                key: key.as_str().to_string(),
                size,
                sha256,
                content_type: options
                    .content_type
                    .clone()
                    .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
                last_modified: Utc::now(),
                metadata: options.metadata.clone(),
            };
            let encoded_len = format::append_footer(&mut file, size, &meta, OBJECT_MAGIC).await?;
            drop(file);
            let final_path = paths.object(key);
            if options.if_none_match {
                paths::commit_exclusive(&staged, &final_path)
                    .await
                    .map_err(|err| match err {
                        BlobError::AlreadyExists(_) => BlobError::AlreadyExists(key.to_string()),
                        other => other,
                    })?;
            } else {
                paths::commit_replace(&staged, &final_path).await?;
            }
            self.cache.insert(bucket, meta.clone(), encoded_len);
            Ok(meta)
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&staged).await;
        }
        if let Ok(meta) = &result {
            tracing::debug!(
                operation = "put",
                bytes = meta.size,
                elapsed_ms = started.elapsed().as_millis(),
                "blob operation completed"
            );
        }
        result
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    fn backend(&self) -> &'static str {
        "fs"
    }

    async fn create_bucket(&self, bucket: &str) -> Result<()> {
        validate_bucket(bucket)?;
        if self.bucket_exists(bucket).await? {
            return Ok(());
        }
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let paths = BucketPaths::new(&self.root, bucket);
        for dir in [paths.objects_root(), paths.uploads_root(), paths.tmp_root()] {
            fs::create_dir_all(&dir)
                .await
                .map_err(|err| BlobError::Io(format!("creating {}: {err}", dir.display())))?;
        }
        fs::write(paths.marker(), paths::V2_MARKER)
            .await
            .map_err(|err| BlobError::Io(format!("creating bucket {bucket}: {err}")))?;
        let created_at = marker_created_at(&paths.marker()).await;
        self.buckets
            .write()
            .map_err(|_| BlobError::Io("bucket cache lock poisoned".into()))?
            .insert(bucket.to_string(), created_at);
        Ok(())
    }

    async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        let blocking_paths = paths.clone();
        let has_objects = tokio::task::spawn_blocking(move || walk::has_objects(&blocking_paths))
            .await
            .map_err(|err| BlobError::Io(format!("checking bucket contents: {err}")))??;
        if has_objects {
            return Err(BlobError::BucketNotEmpty(bucket.to_string()));
        }
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        fs::remove_dir_all(&paths.root)
            .await
            .map_err(|err| BlobError::Io(format!("removing bucket {bucket}: {err}")))?;
        self.buckets
            .write()
            .map_err(|_| BlobError::Io("bucket cache lock poisoned".into()))?
            .remove(bucket);
        self.cache.remove_bucket(bucket);
        Ok(())
    }

    async fn bucket_exists(&self, bucket: &str) -> Result<bool> {
        validate_bucket(bucket)?;
        Ok(self
            .buckets
            .read()
            .map_err(|_| BlobError::Io("bucket cache lock poisoned".into()))?
            .contains_key(bucket))
    }

    async fn list_buckets(&self) -> Result<Vec<BucketSummary>> {
        let mut buckets: Vec<_> = self
            .buckets
            .read()
            .map_err(|_| BlobError::Io("bucket cache lock poisoned".into()))?
            .iter()
            .map(|(name, created_at)| BucketSummary {
                name: name.clone(),
                created_at: *created_at,
            })
            .collect();
        buckets.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(buckets)
    }

    async fn put(
        &self,
        bucket: &str,
        key: &ObjectKey,
        body: Vec<u8>,
        options: PutOptions,
    ) -> Result<ObjectMeta> {
        let length = body.len() as u64;
        let mut reader = std::io::Cursor::new(body);
        self.put_stream(bucket, key, &mut reader, Some(length), options)
            .await
    }

    async fn put_stream(
        &self,
        bucket: &str,
        key: &ObjectKey,
        body: &mut (dyn AsyncRead + Send + Unpin),
        content_length: Option<u64>,
        options: PutOptions,
    ) -> Result<ObjectMeta> {
        let paths = self.bucket(bucket).await?;
        self.write_object_stream(bucket, &paths, key, body, content_length, options)
            .await
    }

    async fn head(&self, bucket: &str, key: &ObjectKey) -> Result<ObjectMeta> {
        let started = std::time::Instant::now();
        let paths = self.bucket(bucket).await?;
        let _guard = self.key_lock(bucket, key).lock().await;
        let (meta, cache_hit) = self.read_meta(bucket, &paths, key).await?;
        tracing::debug!(
            operation = "head",
            bytes = meta.size,
            cache_hit,
            elapsed_ms = started.elapsed().as_millis(),
            "blob operation completed"
        );
        Ok(meta)
    }

    async fn open(
        &self,
        bucket: &str,
        key: &ObjectKey,
        range: Option<ByteRange>,
    ) -> Result<ObjectReader> {
        let started = std::time::Instant::now();
        let paths = self.bucket(bucket).await?;
        let _guard = self.key_lock(bucket, key).lock().await;
        let (meta, cache_hit) = self.read_meta(bucket, &paths, key).await?;
        let path = paths.object(key);
        let mut file = fs::File::open(&path)
            .await
            .map_err(|err| paths::read_error(&path, key.as_str(), err))?;
        let resolved = range.map(|range| range.resolve(meta.size)).transpose()?;
        let (start, length) = resolved
            .map(|range| (range.start, range.length))
            .unwrap_or((0, meta.size));
        if start != 0 {
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
        }
        tracing::debug!(
            operation = "open",
            bytes = length,
            cache_hit,
            elapsed_ms = started.elapsed().as_millis(),
            "blob operation completed"
        );
        Ok(ObjectReader {
            meta,
            range: resolved,
            body: Box::new(file.take(length)),
        })
    }

    async fn delete(&self, bucket: &str, key: &ObjectKey) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let _guard = self.key_lock(bucket, key).lock().await;
        match fs::remove_file(paths.object(key)).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(BlobError::Io(format!("deleting {bucket}/{key}: {err}"))),
        }
        self.cache.remove(bucket, key.as_str());
        Ok(())
    }

    async fn list(&self, bucket: &str, request: &ListRequest) -> Result<ListResponse> {
        let started = std::time::Instant::now();
        let paths = self.bucket(bucket).await?;
        let request = request.clone();
        let cache = self.cache.clone();
        let bucket = bucket.to_string();
        let (response, stats) =
            tokio::task::spawn_blocking(move || walk::page(&bucket, &paths, &request, &cache))
                .await
                .map_err(|err| BlobError::Io(format!("listing task failed: {err}")))??;
        tracing::debug!(
            operation = "list",
            returned = response.objects.len() + response.common_prefixes.len(),
            visited_entries = stats.visited_entries,
            metadata_reads = stats.metadata_reads,
            elapsed_ms = started.elapsed().as_millis(),
            "blob operation completed"
        );
        Ok(response)
    }

    async fn create_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        options: PutOptions,
    ) -> Result<String> {
        let paths = self.bucket(bucket).await?;
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let upload_id = staging_name("upload");
        let dir = paths.upload(&upload_id);
        fs::create_dir_all(&dir)
            .await
            .map_err(|err| BlobError::Io(format!("creating {}: {err}", dir.display())))?;
        let manifest = UploadManifest {
            key: key.as_str().to_string(),
            content_type: options.content_type,
            metadata: options.metadata,
            if_none_match: options.if_none_match,
            expected_sha256: options.expected_sha256,
        };
        let encoded = serde_json::to_vec(&manifest)
            .map_err(|err| BlobError::Io(format!("encoding upload manifest: {err}")))?;
        fs::write(dir.join("upload.json"), encoded)
            .await
            .map_err(|err| BlobError::Io(format!("writing upload manifest: {err}")))?;
        Ok(upload_id)
    }

    async fn upload_part(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        body: Vec<u8>,
    ) -> Result<String> {
        let length = body.len() as u64;
        let mut reader = std::io::Cursor::new(body);
        self.upload_part_stream(
            bucket,
            key,
            upload_id,
            part_number,
            &mut reader,
            Some(length),
            PutOptions::default(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn upload_part_stream(
        &self,
        bucket: &str,
        _key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        body: &mut (dyn AsyncRead + Send + Unpin),
        content_length: Option<u64>,
        options: PutOptions,
    ) -> Result<String> {
        if !(MIN_PART_NUMBER..=MAX_PART_NUMBER).contains(&part_number) {
            return Err(BlobError::BadRequest(format!(
                "part number {part_number} outside {MIN_PART_NUMBER}..={MAX_PART_NUMBER}"
            )));
        }
        let paths = self.bucket(bucket).await?;
        let dir = paths.upload(upload_id);
        if !fs::try_exists(dir.join("upload.json"))
            .await
            .unwrap_or(false)
        {
            return Err(BlobError::NoSuchUpload(upload_id.to_string()));
        }
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let (staged, mut file) =
            paths::create_staged(&paths, &format!("part-{}", uuid::Uuid::now_v7())).await?;
        let result = async {
            let (size, sha256) =
                migrate::copy_hashed(body, &mut file, self.options.io_buffer_bytes).await?;
            if content_length.is_some_and(|expected| expected != size) {
                return Err(BlobError::BadRequest(format!(
                    "part length declared {} bytes but received {size}",
                    content_length.unwrap_or_default()
                )));
            }
            if let Some(expected) = options.expected_sha256 {
                if !expected.eq_ignore_ascii_case(&sha256) {
                    return Err(BlobError::DigestMismatch {
                        expected,
                        actual: sha256,
                    });
                }
            }
            format::append_footer(
                &mut file,
                size,
                &PartMeta {
                    size,
                    sha256: sha256.clone(),
                },
                PART_MAGIC,
            )
            .await?;
            drop(file);
            paths::commit_replace(&staged, &dir.join(part_filename(part_number))).await?;
            Ok(format!("\"{sha256}\""))
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&staged).await;
        }
        result
    }

    async fn complete_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<ObjectMeta> {
        let started = std::time::Instant::now();
        let paths = self.bucket(bucket).await?;
        let dir = paths.upload(upload_id);
        let manifest = fs::read(dir.join("upload.json"))
            .await
            .map_err(|_| BlobError::NoSuchUpload(upload_id.to_string()))?;
        let manifest: UploadManifest = serde_json::from_slice(&manifest)
            .map_err(|err| BlobError::Io(format!("parsing upload manifest: {err}")))?;
        if manifest.key != key.as_str() {
            return Err(BlobError::BadRequest(format!(
                "upload {upload_id} was opened for key '{}', not '{key}'",
                manifest.key
            )));
        }
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let _guard = self.key_lock(bucket, key).lock().await;
        let (staged, mut target) =
            paths::create_staged(&paths, &format!("complete-{}", uuid::Uuid::now_v7())).await?;
        let result = async {
            let mut hasher = Sha256::new();
            let mut size = 0u64;
            let mut previous = 0;
            let mut buffer = vec![0u8; self.options.io_buffer_bytes];
            for part in parts {
                if part.part_number <= previous {
                    return Err(BlobError::BadRequest(
                        "completion parts must be in ascending part-number order".into(),
                    ));
                }
                previous = part.part_number;
                let path = dir.join(part_filename(part.part_number));
                let mut source = fs::File::open(&path).await.map_err(|_| {
                    BlobError::BadRequest(format!("part {} was never uploaded", part.part_number))
                })?;
                let (part_meta, _) = format::read_part(&mut source, &path).await?;
                let actual = format!("\"{}\"", part_meta.sha256);
                if actual != part.etag {
                    return Err(BlobError::DigestMismatch {
                        expected: part.etag.clone(),
                        actual,
                    });
                }
                source
                    .seek(std::io::SeekFrom::Start(0))
                    .await
                    .map_err(|err| BlobError::Io(format!("seeking {}: {err}", path.display())))?;
                let mut remaining = part_meta.size;
                while remaining != 0 {
                    let wanted =
                        usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
                    let read = source
                        .read(&mut buffer[..wanted])
                        .await
                        .map_err(|err| BlobError::Io(format!("reading multipart part: {err}")))?;
                    if read == 0 {
                        return Err(BlobError::Io(format!(
                            "multipart part {} is truncated",
                            part.part_number
                        )));
                    }
                    target.write_all(&buffer[..read]).await.map_err(|err| {
                        BlobError::Io(format!("assembling multipart object: {err}"))
                    })?;
                    hasher.update(&buffer[..read]);
                    size = size
                        .checked_add(read as u64)
                        .ok_or_else(|| BlobError::BadRequest("multipart size overflow".into()))?;
                    remaining -= read as u64;
                }
            }
            let sha256 = hex::encode(hasher.finalize());
            if let Some(expected) = &manifest.expected_sha256 {
                if !expected.eq_ignore_ascii_case(&sha256) {
                    return Err(BlobError::DigestMismatch {
                        expected: expected.clone(),
                        actual: sha256,
                    });
                }
            }
            let meta = ObjectMeta {
                key: key.as_str().to_string(),
                size,
                sha256,
                content_type: manifest
                    .content_type
                    .clone()
                    .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
                last_modified: Utc::now(),
                metadata: manifest.metadata.clone(),
            };
            let encoded_len = format::append_footer(&mut target, size, &meta, OBJECT_MAGIC).await?;
            drop(target);
            if manifest.if_none_match {
                paths::commit_exclusive(&staged, &paths.object(key))
                    .await
                    .map_err(|err| match err {
                        BlobError::AlreadyExists(_) => BlobError::AlreadyExists(key.to_string()),
                        other => other,
                    })?;
            } else {
                paths::commit_replace(&staged, &paths.object(key)).await?;
            }
            self.cache.insert(bucket, meta.clone(), encoded_len);
            Ok(meta)
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&staged).await;
        } else {
            let _ = fs::remove_dir_all(&dir).await;
        }
        if let Ok(meta) = &result {
            tracing::debug!(
                operation = "complete_multipart",
                bytes = meta.size,
                parts = parts.len(),
                elapsed_ms = started.elapsed().as_millis(),
                "blob operation completed"
            );
        }
        result
    }

    async fn abort_multipart(&self, bucket: &str, _key: &ObjectKey, upload_id: &str) -> Result<()> {
        let paths = self.bucket(bucket).await?;
        let _permit = self
            .mutations
            .acquire()
            .await
            .map_err(|_| BlobError::Io("blob write limiter closed".into()))?;
        let _ = fs::remove_dir_all(paths.upload(upload_id)).await;
        Ok(())
    }
}
