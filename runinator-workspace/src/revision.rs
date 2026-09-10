//! Named results and provider directories over the immutable storage core.
#[cfg(test)]
#[path = "revision_tests.rs"]
mod tests;

use crate::storage::{
    self, Layout, Metadata,
    cache::ByteCache,
    model::{FileObject, InodeData, Kind},
    pages, radix,
    store::{ReadStore, WriteStore, load},
    transaction::Edit,
    view::View,
};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::{WorkspaceLimits, WorkspaceUsage},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Seek, SeekFrom, Write},
    path::Path,
};

struct Count(u64);
impl Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| std::io::Error::other("JSON size overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Canonical result values are paged file objects under the revision attachment radix.
pub fn save_results<S: WriteStore>(
    edit: &mut Edit<S>,
    results: &BTreeMap<String, Value>,
    limits: WorkspaceLimits,
    scratch: &Path,
) -> Result<u64, SendableError> {
    let mut count = Count(0);
    serde_json::to_writer(&mut count, results)?;
    limits.check(WorkspaceUsage {
        logical_bytes: count.0,
        results_bytes: count.0,
        entries: 0,
    })?;
    let mut root = None;
    for (name, value) in results {
        if name.is_empty() || name.len() > 4096 {
            return Err(WORKSPACE_INVALID.error("result names must contain 1 to 4096 UTF-8 bytes"));
        }
        let mut file = tempfile::tempfile_in(scratch)?;
        serde_json::to_writer(&mut file, value)?;
        let length = file.stream_position()?;
        file.seek(SeekFrom::Start(0))?;
        let id = pages::ingest(&edit.store, Layout::for_size(length), file)?;
        root = radix::set(&edit.store, root, name.as_bytes(), Some(id))?;
    }
    edit.attachments = root;
    Ok(count.0)
}

pub fn read_result<S: ReadStore>(view: &View<S>, name: &str) -> Result<Value, SendableError> {
    let id = radix::get(&view.store, view.attachments, name.as_bytes())?
        .ok_or_else(|| WORKSPACE_INVALID.error("named result not found"))?;
    let cache = ByteCache::new(8 * 1024 * 1024);
    let reader = pages::FileReader::new(&view.store, &cache, id)?;
    Ok(serde_json::from_reader(std::io::BufReader::with_capacity(
        64 * 1024,
        reader,
    ))?)
}

pub fn read_results<S: ReadStore>(
    view: &View<S>,
) -> Result<BTreeMap<String, Value>, SendableError> {
    let mut results = BTreeMap::new();
    let mut after: Option<Vec<u8>> = None;
    loop {
        let batch = radix::page(&view.store, view.attachments, after.as_deref(), 256)?;
        if batch.is_empty() {
            break;
        }
        for (key, _) in batch {
            let name = std::str::from_utf8(&key)?;
            results.insert(name.into(), read_result(view, name)?);
            after = Some(key);
        }
    }
    Ok(results)
}

pub fn validate_results<S: ReadStore>(view: &View<S>) -> Result<(), SendableError> {
    use serde::Deserialize;
    let cache = ByteCache::new(8 * 1024 * 1024);
    radix::visit(&view.store, view.attachments, &mut |name, id| {
        if name.is_empty() || name.len() > 4096 || std::str::from_utf8(name).is_err() {
            return Err(storage::Error::Invalid("invalid result name".into()));
        }
        let reader = std::io::BufReader::with_capacity(
            64 * 1024,
            pages::FileReader::new(&view.store, &cache, id)?,
        );
        let mut deserializer = serde_json::Deserializer::from_reader(reader);
        serde::de::IgnoredAny::deserialize(&mut deserializer)?;
        deserializer.end()?;
        Ok(())
    })?;
    Ok(())
}

pub fn validate_links<S: ReadStore>(view: &View<S>) -> Result<(), SendableError> {
    fn walk<S: ReadStore>(
        view: &View<S>,
        path: &str,
        links: &mut BTreeMap<std::path::PathBuf, std::path::PathBuf>,
    ) -> Result<(), SendableError> {
        let mut after = None;
        loop {
            let entries = view.directory(path, after.as_deref(), 256)?;
            if entries.is_empty() {
                break;
            }
            for entry in entries {
                let child = if path.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{path}/{}", entry.name)
                };
                match entry.inode.data {
                    InodeData::Directory(_) => walk(view, &child, links)?,
                    InodeData::Symlink(target) => {
                        super::validate_link(Path::new(&child), Path::new(&target))?;
                        links.insert(child.into(), target.into());
                    }
                    _ => {}
                }
                after = Some(entry.name);
            }
        }
        Ok(())
    }
    let mut links = BTreeMap::new();
    walk(view, "", &mut links)?;
    for (path, target) in &links {
        super::validate_link_graph(path, target, &links)?;
    }
    Ok(())
}

/// Scan every file's bytes; timestamps are metadata and never evidence of unchanged content.
pub fn capture<S: WriteStore>(
    store: S,
    root: &Path,
    results: &BTreeMap<String, Value>,
    limits: WorkspaceLimits,
    scratch: &Path,
) -> Result<(Edit<S>, WorkspaceUsage), SendableError> {
    capture_from(store, None, root, results, limits, scratch)
}

/// Reconcile against an immutable base while scanning all current content.
pub fn capture_from<S: WriteStore>(
    store: S,
    base: Option<storage::Id>,
    root: &Path,
    results: &BTreeMap<String, Value>,
    limits: WorkspaceLimits,
    scratch: &Path,
) -> Result<(Edit<S>, WorkspaceUsage), SendableError> {
    limits.validate()?;
    let root = root.canonicalize()?;
    let mut edit = Edit::new(store, base, Layout::default(), 16 * 1024 * 1024)?;
    if base.is_some() {
        super::reconcile::prepare(&mut edit, &root)?;
    }
    edit.set_metadata("", stored_metadata(&fs::metadata(&root)?))?;
    let results_bytes = save_results(&mut edit, results, limits, scratch)?;
    let mut usage = WorkspaceUsage {
        logical_bytes: results_bytes,
        results_bytes,
        entries: 0,
    };
    let mut links = BTreeMap::new();
    scan(&mut edit, &root, "", limits, &mut usage, &mut links)?;
    Ok((edit, usage))
}

/// Reuse an immutable base namespace while replacing only its named results.
pub fn checkpoint_results<S: WriteStore>(
    store: S,
    base: storage::Id,
    base_usage: WorkspaceUsage,
    results: &BTreeMap<String, Value>,
    limits: WorkspaceLimits,
    scratch: &Path,
) -> Result<(Edit<S>, WorkspaceUsage), SendableError> {
    limits.validate()?;
    let mut edit = Edit::new(store, Some(base), Layout::default(), 16 * 1024 * 1024)?;
    let results_bytes = save_results(&mut edit, results, limits, scratch)?;
    let logical_bytes = base_usage
        .logical_bytes
        .checked_sub(base_usage.results_bytes)
        .and_then(|bytes| bytes.checked_add(results_bytes))
        .ok_or_else(|| WORKSPACE_INVALID.error("workspace result size overflow"))?;
    let usage = WorkspaceUsage {
        logical_bytes,
        results_bytes,
        entries: base_usage.entries,
    };
    limits.check(usage)?;
    Ok((edit, usage))
}

fn scan<S: WriteStore>(
    edit: &mut Edit<S>,
    root: &Path,
    relative: &str,
    limits: WorkspaceLimits,
    usage: &mut WorkspaceUsage,
    links: &mut BTreeMap<(u64, u64), String>,
) -> Result<(), SendableError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root.join(relative))? {
        usage.entries = usage
            .entries
            .checked_add(1)
            .ok_or_else(|| WORKSPACE_INVALID.error("entry count overflow"))?;
        limits.check(*usage)?;
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| WORKSPACE_INVALID.error("workspace names must be UTF-8"))?;
        let path = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        super::validate_path(Path::new(&path))?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            if edit.stat(&path).is_err() {
                edit.mkdir(&path)?;
            }
            scan(edit, root, &path, limits, usage, links)?;
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            super::validate_link(Path::new(&path), &target)?;
            let target = target
                .to_str()
                .ok_or_else(|| WORKSPACE_INVALID.error("workspace link targets must be UTF-8"))?;
            if edit.stat(&path).is_err() {
                edit.symlink(&path, target)?;
            }
        } else if metadata.is_file() {
            let identity = file_identity(&metadata);
            if let Some(source) = identity.and_then(|key| links.get(&key)) {
                let source_number = edit.stat(source)?.0;
                match edit.stat(&path) {
                    Ok((number, _)) if number == source_number => {}
                    Ok(_) => {
                        edit.unlink(&path)?;
                        edit.hard_link(source, &path)?;
                    }
                    Err(storage::Error::NotFound(_)) => edit.hard_link(source, &path)?,
                    Err(error) => return Err(error.into()),
                }
            } else {
                usage.logical_bytes = usage
                    .logical_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| WORKSPACE_INVALID.error("workspace size overflow"))?;
                limits.check(*usage)?;
                let file = fs::File::open(entry.path())?;
                // check the opened file before consuming bytes, including replacement races.
                let opened = file.metadata()?;
                if !opened.is_file()
                    || opened.len() != metadata.len()
                    || file_identity(&opened) != identity
                {
                    return Err(WORKSPACE_INVALID.error("workspace changed during capture"));
                }
                edit.put_sized(&path, &file, metadata.len())?;
                if file.metadata()?.modified()? != opened.modified()?
                    || file.metadata()?.len() != opened.len()
                {
                    return Err(WORKSPACE_INVALID.error("workspace changed during capture"));
                }
                if let Some(identity) = identity {
                    links.insert(identity, path.clone());
                }
            }
        } else {
            return Err(WORKSPACE_INVALID.error("workspace contains a special file"));
        }
        edit.set_metadata(&path, stored_metadata(&metadata))?;
    }
    Ok(())
}

pub(crate) fn file_identity(metadata: &fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn stored_metadata(metadata: &fs::Metadata) -> Metadata {
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            duration.as_nanos().min(i64::MAX as u128) as i64
        });
    Metadata {
        mode,
        modified_ns,
        ..Metadata::default()
    }
}

pub fn materialize<S: ReadStore>(view: &View<S>, root: &Path) -> Result<(), SendableError> {
    if fs::read_dir(root)?.next().is_some() {
        return Err(WORKSPACE_INVALID.error("restore directory must be empty"));
    }
    let mut hardlinks = BTreeMap::new();
    let mut symlinks = BTreeMap::new();
    restore_directory(view, root, "", &mut hardlinks, &mut symlinks)?;
    for (path, target) in &symlinks {
        super::validate_link_graph(path, target, &symlinks)?;
    }
    for (path, target) in symlinks {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, root.join(path))?;
        #[cfg(not(unix))]
        {
            let _ = (path, target);
            return Err(WORKSPACE_INVALID
                .error("symbolic link restoration is unsupported on this platform"));
        }
    }
    restore_metadata(view, root, "")?;
    Ok(())
}

fn restore_metadata<S: ReadStore>(
    view: &View<S>,
    root: &Path,
    relative: &str,
) -> Result<(), SendableError> {
    let (_, inode) = view.stat(relative)?;
    if matches!(inode.data, InodeData::Directory(_)) {
        let mut after = None;
        loop {
            let entries = view.directory(relative, after.as_deref(), 256)?;
            if entries.is_empty() {
                break;
            }
            for entry in entries {
                let path = if relative.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{relative}/{}", entry.name)
                };
                restore_metadata(view, root, &path)?;
                after = Some(entry.name);
            }
        }
    }
    let path = root.join(relative);
    let ns = inode.metadata.modified_ns;
    let time = filetime::FileTime::from_unix_time(
        ns.div_euclid(1_000_000_000),
        ns.rem_euclid(1_000_000_000) as u32,
    );
    if matches!(inode.data, InodeData::Symlink(_)) {
        filetime::set_symlink_file_times(path, time, time)?;
    } else {
        filetime::set_file_mtime(&path, time)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(inode.metadata.mode & 0o777),
            )?;
        }
    }
    Ok(())
}

fn restore_directory<S: ReadStore>(
    view: &View<S>,
    root: &Path,
    relative: &str,
    hardlinks: &mut BTreeMap<u64, std::path::PathBuf>,
    symlinks: &mut BTreeMap<std::path::PathBuf, std::path::PathBuf>,
) -> Result<(), SendableError> {
    let mut after = None;
    loop {
        let entries = view.directory(relative, after.as_deref(), 256)?;
        if entries.is_empty() {
            break;
        }
        for entry in entries {
            let path = if relative.is_empty() {
                entry.name.clone()
            } else {
                format!("{relative}/{}", entry.name)
            };
            let path = Path::new(&path);
            super::validate_path(path)?;
            let dest = root.join(path);
            match &entry.inode.data {
                InodeData::Directory(_) => {
                    fs::create_dir(&dest)?;
                    restore_directory(
                        view,
                        root,
                        path.to_str()
                            .ok_or_else(|| WORKSPACE_INVALID.error("invalid path"))?,
                        hardlinks,
                        symlinks,
                    )?;
                }
                InodeData::File(_) => {
                    if let Some(source) = hardlinks.get(&entry.inode_number) {
                        fs::hard_link(source, &dest)?;
                    } else {
                        let mut file = fs::OpenOptions::new()
                            .create_new(true)
                            .write(true)
                            .open(&dest)?;
                        view.materialize_file(
                            path.to_str()
                                .ok_or_else(|| WORKSPACE_INVALID.error("invalid path"))?,
                            &mut file,
                        )?;
                        file.flush()?;
                        hardlinks.insert(entry.inode_number, dest.clone());
                    }
                }
                InodeData::Symlink(target) => {
                    let target = Path::new(target);
                    super::validate_link(path, target)?;
                    symlinks.insert(path.into(), target.into());
                }
            }
            after = Some(entry.name);
        }
    }
    Ok(())
}

/// Account verified namespace inodes exactly once, including holes and all directory entries.
pub fn usage<S: ReadStore>(view: &View<S>) -> Result<WorkspaceUsage, SendableError> {
    let mut usage = WorkspaceUsage::default();
    radix::visit(&view.store, view.workspace.inodes, &mut |_, id| {
        let inode: storage::model::Inode = load(&view.store, id, Kind::Inode)?;
        match inode.data {
            InodeData::File(file) => {
                let file: FileObject = load(&view.store, file, Kind::File)?;
                usage.logical_bytes = usage
                    .logical_bytes
                    .checked_add(file.size)
                    .ok_or_else(|| storage::Error::Invalid("size overflow".into()))?;
            }
            InodeData::Directory(root) => radix::visit(&view.store, root, &mut |_, _| {
                usage.entries = usage
                    .entries
                    .checked_add(1)
                    .ok_or_else(|| storage::Error::Invalid("entry overflow".into()))?;
                Ok(())
            })?,
            _ => {}
        }
        Ok(())
    })?;
    // include the enclosing canonical JSON map and encoded result names without loading values.
    usage.results_bytes = 2;
    let mut first = true;
    radix::visit(&view.store, view.attachments, &mut |name, id| {
        let name = std::str::from_utf8(name)
            .map_err(|_| storage::Error::Invalid("invalid result name".into()))?;
        let name = serde_json::to_vec(name)?;
        let file: FileObject = load(&view.store, id, Kind::File)?;
        usage.results_bytes = usage
            .results_bytes
            .checked_add(file.size)
            .and_then(|n| n.checked_add(name.len() as u64 + 1 + u64::from(!first)))
            .ok_or_else(|| storage::Error::Invalid("result size overflow".into()))?;
        first = false;
        Ok(())
    })?;
    usage.logical_bytes = usage
        .logical_bytes
        .checked_add(usage.results_bytes)
        .ok_or_else(|| WORKSPACE_INVALID.error("workspace size overflow"))?;
    Ok(usage)
}
