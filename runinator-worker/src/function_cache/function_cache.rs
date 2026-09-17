#[allow(unused_imports)]
use super::*;

pub struct FunctionCache {
    pub(super) client: Arc<dyn FunctionArtifactSource>,
    pub(super) root: PathBuf,
    pub(super) capacity_bytes: u64,
}

impl FunctionCache {
    pub fn new(client: impl FunctionArtifactSource + 'static) -> Self {
        let root = app_data::app_data_path("worker/functions")
            .unwrap_or_else(|_| std::env::temp_dir().join("runinator-worker-functions"));
        Self {
            client: Arc::new(client),
            root,
            capacity_bytes: DEFAULT_CACHE_BYTES,
        }
    }

    pub fn with_root(client: impl FunctionArtifactSource + 'static, root: PathBuf) -> Self {
        Self {
            client: Arc::new(client),
            root,
            capacity_bytes: DEFAULT_CACHE_BYTES,
        }
    }

    pub fn with_capacity(mut self, capacity_bytes: u64) -> Self {
        self.capacity_bytes = capacity_bytes;
        self
    }

    /// the directory a digest's code is staged in, downloading and unpacking it if needed.
    pub async fn stage(&self, digest: &str) -> Result<PathBuf, SendableError> {
        if !is_valid_digest(digest) {
            return Err(
                FUNCTION_UNTRUSTED_ARCHIVE.error(format!("'{digest}' is not a sha256 digest"))
            );
        }
        let hex = digest.trim_start_matches("sha256:");
        let staged = self.root.join(hex);
        if staged.join(READY_MARKER).is_file() {
            // touch the marker so eviction sees this as recently used; the code itself is never
            // rewritten, so its own mtime would only ever say when it was first fetched.
            let _ = filetime_touch(&staged.join(READY_MARKER));
            debug!(digest = %digest, "packaged function already staged");
            return Ok(staged);
        }

        let bytes = self.client.download_function_artifact(digest).await?;
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(FUNCTION_STAGING_FAILED.error(format!(
                "artifact {digest} is {} bytes, over the {MAX_ARCHIVE_BYTES} limit",
                bytes.len()
            )));
        }
        // the digest is re-derived from what actually arrived. everything downstream — the pinning
        // guarantee, the cache key, "this workflow runs exactly this code" — rests on the bytes
        // matching, and trusting the server to have checked would make this the one place a
        // corrupted or substituted archive could enter execution unnoticed.
        let actual = runinator_models::functions::digest_from_hex(&sha256_hex(&bytes));
        if actual != digest {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "artifact bytes hash to {actual}, not the requested {digest}"
            )));
        }

        self.evict_to_fit(bytes.len() as u64);
        unpack(&bytes, &staged)?;
        std::fs::write(staged.join(READY_MARKER), digest).map_err(|err| {
            FUNCTION_STAGING_FAILED.error(format!("failed to mark {digest} staged: {err}"))
        })?;
        debug!(digest = %digest, path = %staged.display(), "staged packaged function");
        Ok(staged)
    }

    // drop least-recently-used entries until the incoming archive fits. eviction is safe at any
    // moment: an entry is only ever a cache of bytes that can be fetched again, and a concurrent
    // invocation holds an open mount rather than depending on the directory entry.
    pub(super) fn evict_to_fit(&self, incoming: u64) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        let mut staged: Vec<(SystemTime, u64, PathBuf)> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| {
                let path = entry.path();
                let used = last_used(&path);
                (used, directory_size(&path), path)
            })
            .collect();
        let mut total: u64 = staged.iter().map(|(_, size, _)| size).sum();
        if total + incoming <= self.capacity_bytes {
            return;
        }
        staged.sort_by_key(|(used, _, _)| *used);
        for (_, size, path) in staged {
            if total + incoming <= self.capacity_bytes {
                break;
            }
            if std::fs::remove_dir_all(&path).is_ok() {
                total = total.saturating_sub(size);
                debug!(path = %path.display(), "evicted staged packaged function");
            }
        }
    }
}
