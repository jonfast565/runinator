//! staging packaged-function code on the worker.
//!
//! an action carrying a `FunctionBinding` names code by digest, not by location. before the
//! provider runs, that code has to exist as a directory on this machine — so this downloads the
//! artifact, **verifies the digest against the bytes**, unpacks it, and hands back the path.
//!
//! it lives in the worker rather than the provider on purpose. the provider is given a local path
//! and mounts it; it makes no control-plane calls at all. that split is what lets the same provider
//! run on a host worker, on the desktop agent, or (later) inside a kubernetes job, each of which
//! gets the bytes a different way.
//!
//! the cache is keyed by digest, which makes it trivially correct: the same digest is the same
//! bytes by construction, so a hit never needs revalidating and two versions sharing an artifact
//! share one staged copy.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_models::functions::{FunctionBinding, is_valid_digest};
use runinator_models::value::{Map, Value};
use runinator_platform::app_data;
use tracing::debug;

use crate::errors::{
    FUNCTION_BINDING_UNRESOLVED, FUNCTION_STAGING_FAILED, FUNCTION_UNTRUSTED_ARCHIVE,
};

/// the largest package archive that will be staged.
pub const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum expanded size of one file inside a packaged-function archive.
pub const MAX_UNPACKED_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum aggregate expanded size of a packaged-function archive.
pub const MAX_UNPACKED_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// Maximum files/directories one packaged-function archive may contain.
pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;

/// how much unpacked code one cache may hold before the least recently used entries are evicted.
pub const DEFAULT_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// downloads and unpacks packaged-function artifacts, keyed by digest.
pub struct FunctionCache {
    client: AsyncApiClient<StaticLocator>,
    root: PathBuf,
    capacity_bytes: u64,
}

impl FunctionCache {
    pub fn new(client: AsyncApiClient<StaticLocator>) -> Self {
        let root = app_data::app_data_path("worker/functions")
            .unwrap_or_else(|_| std::env::temp_dir().join("runinator-worker-functions"));
        Self {
            client,
            root,
            capacity_bytes: DEFAULT_CACHE_BYTES,
        }
    }

    pub fn with_root(client: AsyncApiClient<StaticLocator>, root: PathBuf) -> Self {
        Self {
            client,
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
    fn evict_to_fit(&self, incoming: u64) {
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

/// where the authored arguments live on a packaged-function action.
pub const INPUT_KEY: &str = "input";

/// written last, so a directory without it is a partial unpack rather than a usable cache entry.
const READY_MARKER: &str = ".runinator-staged";

/// stage an action's packaged function and build the parameters the provider reads.
///
/// two things are resolved from the *published* version rather than from anything the dispatch
/// carried: the digest the code is fetched by, and the handler/runtime/limits it runs under. that is
/// what keeps an alias movement from changing how an already-compiled workflow behaves — the
/// binding names an exact version, and this reads that version.
pub async fn prepare_invocation(
    cache: &FunctionCache,
    client: &AsyncApiClient<StaticLocator>,
    binding: &FunctionBinding,
    authored: Value,
    context: Value,
) -> Result<Value, SendableError> {
    let target = client
        .resolve_function_export(binding.export_id)
        .await
        .map_err(|err| {
            FUNCTION_BINDING_UNRESOLVED.error(format!(
                "could not resolve {} (export {}): {err}",
                binding.call_path(),
                binding.export_id
            ))
        })?;
    // the binding's digest wins over the resolved one if they ever disagree: the binding is what
    // the workflow was compiled against, and a mismatch means the server answered about different
    // code than the action asked for.
    if target.artifact_digest != binding.artifact_digest {
        return Err(FUNCTION_BINDING_UNRESOLVED.error(format!(
            "{} resolved to artifact {} but the action pins {}",
            binding.call_path(),
            target.artifact_digest,
            binding.artifact_digest
        )));
    }
    let package_path = cache.stage(&binding.artifact_digest).await?;

    let mut parameters = Map::new();
    parameters.insert(
        "package_path".into(),
        Value::String(package_path.to_string_lossy().into_owned()),
    );
    parameters.insert(
        "handler".into(),
        Value::String(target.export.handler.clone()),
    );
    parameters.insert(
        "runtime".into(),
        serde_json::to_value(&target.runtime)
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    parameters.insert(
        "limits".into(),
        serde_json::to_value(&target.export.limits)
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    parameters.insert("input".into(), authored);
    parameters.insert("context".into(), context);
    Ok(Value::Object(parameters))
}

// unpack a zip into `target`, refusing any entry that would write outside it.
fn unpack(bytes: &[u8], target: &Path) -> Result<(), SendableError> {
    unpack_with_limits(bytes, target, UnpackLimits::default())
}

#[derive(Clone, Copy)]
struct UnpackLimits {
    entries: usize,
    entry_bytes: u64,
    total_bytes: u64,
}

impl Default for UnpackLimits {
    fn default() -> Self {
        Self {
            entries: MAX_ARCHIVE_ENTRIES,
            entry_bytes: MAX_UNPACKED_ENTRY_BYTES,
            total_bytes: MAX_UNPACKED_ARCHIVE_BYTES,
        }
    }
}

fn unpack_with_limits(
    bytes: &[u8],
    target: &Path,
    limits: UnpackLimits,
) -> Result<(), SendableError> {
    // staged under a temporary name and renamed, so an interrupted unpack never leaves a directory
    // that looks complete. the ready marker is the second half of that guarantee.
    let staging = target.with_extension(format!("partial-{}", uuid::Uuid::new_v4().simple()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|err| {
        FUNCTION_STAGING_FAILED.error(format!("failed to create staging directory: {err}"))
    })?;

    if let Err(err) = unpack_into(bytes, &staging, limits) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    let _ = std::fs::remove_dir_all(target);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::rename(&staging, target).map_err(|err| {
        let _ = std::fs::remove_dir_all(&staging);
        FUNCTION_STAGING_FAILED.error(format!("failed to publish staged package: {err}"))
    })
}

fn unpack_into(bytes: &[u8], staging: &Path, limits: UnpackLimits) -> Result<(), SendableError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|err| {
        FUNCTION_UNTRUSTED_ARCHIVE.error(format!("not a readable archive: {err}"))
    })?;
    if archive.len() > limits.entries {
        return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
            "archive has {} entries, limit is {}",
            archive.len(),
            limits.entries
        )));
    }

    // Validate the complete central directory before writing a byte. The read loop enforces the
    // same byte limits again because an attacker controls the advertised sizes too.
    let mut paths = HashSet::new();
    let mut advertised_total = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| {
            FUNCTION_UNTRUSTED_ARCHIVE.error(format!("unreadable archive entry: {err}"))
        })?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive entry '{}' escapes the package directory",
                entry.name()
            )));
        };
        let normalized = relative.to_string_lossy().to_ascii_lowercase();
        if !paths.insert(normalized) {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive contains duplicate or case-colliding path '{}'",
                relative.display()
            )));
        }
        if entry.is_dir() {
            continue;
        }
        if entry.size() > limits.entry_bytes {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive entry '{}' expands to {} bytes, per-entry limit is {}",
                relative.display(),
                entry.size(),
                limits.entry_bytes
            )));
        }
        advertised_total = advertised_total.checked_add(entry.size()).ok_or_else(|| {
            FUNCTION_UNTRUSTED_ARCHIVE.error("archive expanded size overflows u64")
        })?;
        if advertised_total > limits.total_bytes {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive expands to more than {} bytes",
                limits.total_bytes
            )));
        }
    }

    let mut remaining = limits.total_bytes;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| {
            FUNCTION_UNTRUSTED_ARCHIVE.error(format!("unreadable archive entry: {err}"))
        })?;
        // `enclosed_name` is what rejects `../` and absolute paths: a zip is attacker-controlled
        // input, and an entry that escaped the staging directory would write anywhere the worker
        // can. an entry it refuses fails the staging rather than being skipped, because a package
        // that tried is not one to run a subset of.
        let Some(relative) = entry.enclosed_name() else {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive entry '{}' escapes the package directory",
                entry.name()
            )));
        };
        let path = staging.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|err| {
                FUNCTION_STAGING_FAILED.error(format!("failed to create {}: {err}", path.display()))
            })?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                FUNCTION_STAGING_FAILED
                    .error(format!("failed to create {}: {err}", parent.display()))
            })?;
        }
        let read_limit = remaining.min(limits.entry_bytes);
        let mut output = std::fs::File::create(&path).map_err(|err| {
            FUNCTION_STAGING_FAILED.error(format!("failed to create {}: {err}", path.display()))
        })?;
        let copied =
            std::io::copy(&mut (&mut entry).take(read_limit + 1), &mut output).map_err(|err| {
                FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                    "failed to expand archive entry '{}': {err}",
                    path.display()
                ))
            })?;
        if copied > read_limit {
            return Err(FUNCTION_UNTRUSTED_ARCHIVE.error(format!(
                "archive entry '{}' exceeds its remaining expanded-size budget of {read_limit} bytes",
                entry.name()
            )));
        }
        remaining -= copied;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn last_used(path: &Path) -> SystemTime {
    std::fs::metadata(path.join(READY_MARKER))
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => directory_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

// rewriting the marker is the portable way to bump its mtime; the payload is one line either way.
fn filetime_touch(path: &Path) -> std::io::Result<()> {
    let contents = std::fs::read(path)?;
    std::fs::write(path, contents)
}

#[cfg(test)]
#[path = "function_cache_tests.rs"]
mod tests;
