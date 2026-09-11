//! ordered and prefix-pruned traversal for the v2 object tree.

use std::collections::BTreeSet;
use std::path::Path;

use crate::errors::BlobError;
use crate::listing::{ListRequest, ListResponse, ObjectSummary};

use super::cache::MetadataCache;
use super::format;
use super::paths::BucketPaths;

#[derive(Default)]
pub(super) struct ListingStats {
    pub(super) visited_entries: u64,
    pub(super) metadata_reads: u64,
}

pub(super) fn page(
    bucket: &str,
    paths: &BucketPaths,
    request: &ListRequest,
    cache: &MetadataCache,
) -> Result<(ListResponse, ListingStats), BlobError> {
    let prefix = request.prefix.as_deref().unwrap_or("");
    let after = request.continuation_token.as_deref();
    let limit = request.effective_max_keys();
    if limit == 0 {
        return Ok((ListResponse::default(), ListingStats::default()));
    }
    let mut state = PageState {
        bucket,
        request,
        cache,
        objects: Vec::new(),
        common_prefixes: BTreeSet::new(),
        last_seen: None,
        truncated: false,
        stats: ListingStats::default(),
    };
    visit_directory(&paths.objects_root(), "", prefix, after, limit, &mut state)?;
    let response = ListResponse {
        objects: state.objects,
        common_prefixes: state.common_prefixes.into_iter().collect(),
        is_truncated: state.truncated,
        next_continuation_token: state.truncated.then_some(state.last_seen).flatten(),
    };
    Ok((response, state.stats))
}

struct PageState<'a> {
    bucket: &'a str,
    request: &'a ListRequest,
    cache: &'a MetadataCache,
    objects: Vec<ObjectSummary>,
    common_prefixes: BTreeSet<String>,
    last_seen: Option<String>,
    truncated: bool,
    stats: ListingStats,
}

fn visit_directory(
    dir: &Path,
    logical_prefix: &str,
    requested_prefix: &str,
    after: Option<&str>,
    limit: usize,
    state: &mut PageState<'_>,
) -> Result<(), BlobError> {
    if state.truncated {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(BlobError::Io(format!("listing {}: {err}", dir.display()))),
    };
    let mut entries = entries
        .map(|entry| {
            let entry =
                entry.map_err(|err| BlobError::Io(format!("listing {}: {err}", dir.display())))?;
            let file_type = entry.file_type().map_err(|err| {
                BlobError::Io(format!("stating {}: {err}", entry.path().display()))
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let logical = if file_type.is_dir() {
                format!("{name}/")
            } else {
                name.strip_suffix(".blob").unwrap_or(&name).to_string()
            };
            Ok((logical, entry.path(), file_type.is_dir()))
        })
        .collect::<Result<Vec<_>, BlobError>>()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    for (name, path, is_dir) in entries {
        state.stats.visited_entries += 1;
        let logical = format!("{logical_prefix}{name}");
        if is_dir {
            if !subtree_may_match(&logical, requested_prefix, after) {
                continue;
            }
            visit_directory(&path, &logical, requested_prefix, after, limit, state)?;
            if state.truncated {
                return Ok(());
            }
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("blob")
            || !logical.starts_with(requested_prefix)
            || after.is_some_and(|token| logical.as_str() <= token)
        {
            continue;
        }
        if let Some(rolled) = roll_up(
            &logical,
            requested_prefix,
            state.request.delimiter.as_deref(),
        ) {
            if state.common_prefixes.contains(&rolled) {
                state.last_seen = Some(logical);
                continue;
            }
            if state.objects.len() + state.common_prefixes.len() >= limit {
                state.truncated = true;
                return Ok(());
            }
            state.common_prefixes.insert(rolled);
            state.last_seen = Some(logical);
            continue;
        }
        if state.objects.len() + state.common_prefixes.len() >= limit {
            state.truncated = true;
            return Ok(());
        }
        let Some(object) = summary(state, &logical, &path)? else {
            continue;
        };
        state.objects.push(object);
        state.last_seen = Some(logical);
    }
    Ok(())
}

fn subtree_may_match(dir_prefix: &str, requested_prefix: &str, after: Option<&str>) -> bool {
    if !requested_prefix.starts_with(dir_prefix) && !dir_prefix.starts_with(requested_prefix) {
        return false;
    }
    let Some(after) = after else {
        return true;
    };
    after.starts_with(dir_prefix) || after < dir_prefix
}

fn roll_up(key: &str, prefix: &str, delimiter: Option<&str>) -> Option<String> {
    let delimiter = delimiter?;
    if delimiter.is_empty() {
        return None;
    }
    let rest = key.strip_prefix(prefix)?;
    let at = rest.find(delimiter)?;
    Some(format!("{prefix}{}{delimiter}", &rest[..at]))
}

fn summary(
    state: &mut PageState<'_>,
    key: &str,
    path: &Path,
) -> Result<Option<ObjectSummary>, BlobError> {
    let meta = if let Some(meta) = state.cache.get(state.bucket, key) {
        meta
    } else {
        state.stats.metadata_reads += 1;
        let (meta, _encoded_len) = match format::read_object_sync(path) {
            Ok(meta) => meta,
            Err(BlobError::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if meta.key != key {
            return Err(BlobError::Io(format!(
                "blob metadata key '{}' disagrees with path '{key}'",
                meta.key
            )));
        }
        std::sync::Arc::new(meta)
    };
    Ok(Some(ObjectSummary {
        key: meta.key.clone(),
        size: meta.size,
        sha256: meta.sha256.clone(),
        last_modified: meta.last_modified,
    }))
}

pub(super) fn collect_legacy_keys(paths: &BucketPaths) -> Result<Vec<String>, BlobError> {
    let root = paths.legacy_meta_root();
    let mut keys = Vec::new();
    walk_legacy(&root, &root, &mut keys)?;
    keys.sort();
    Ok(keys)
}

fn walk_legacy(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), BlobError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(BlobError::Io(format!("listing {}: {err}", dir.display()))),
    };
    for entry in entries {
        let entry =
            entry.map_err(|err| BlobError::Io(format!("listing {}: {err}", dir.display())))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| BlobError::Io(format!("stating {}: {err}", path.display())))?;
        if file_type.is_dir() {
            walk_legacy(root, &path, out)?;
            continue;
        }
        let Some(relative) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
            continue;
        };
        if let Some(key) = relative.strip_suffix(".json") {
            out.push(key.replace(std::path::MAIN_SEPARATOR, "/"));
        }
    }
    Ok(())
}

pub(super) fn has_objects(paths: &BucketPaths) -> Result<bool, BlobError> {
    fn any(dir: &Path) -> Result<bool, BlobError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(BlobError::Io(format!("listing {}: {err}", dir.display()))),
        };
        for entry in entries {
            let entry =
                entry.map_err(|err| BlobError::Io(format!("listing {}: {err}", dir.display())))?;
            if entry
                .file_type()
                .map_err(|err| BlobError::Io(format!("stating {}: {err}", entry.path().display())))?
                .is_dir()
            {
                if any(&entry.path())? {
                    return Ok(true);
                }
            } else if entry.path().extension().and_then(|value| value.to_str()) == Some("blob") {
                return Ok(true);
            }
        }
        Ok(false)
    }
    any(&paths.objects_root())
}
