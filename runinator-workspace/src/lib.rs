//! Bounded, portable workspace archives shared by workers and storage services.
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::WorkspaceFile,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
pub mod errors;
mod results;
pub use results::resolve_results;

pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 100_000;
const RESULTS_PATH: &str = "results.json";

pub struct PackedWorkspace {
    pub bytes: Vec<u8>,
    pub files: Vec<WorkspaceFile>,
    pub sha256: String,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn pack(
    root: &Path,
    results: &BTreeMap<String, Value>,
) -> Result<PackedWorkspace, SendableError> {
    let root = root.canonicalize()?;
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    archive.follow_symlinks(false);
    let mut files = Vec::new();
    let mut size = 0;
    let mut count = 1;
    append_directory(
        &root,
        &root,
        &mut archive,
        &mut files,
        &mut size,
        &mut count,
    )?;
    let results = serde_json::to_vec(results)?;
    if results.len() > 16 * 1024 * 1024 {
        return Err(WORKSPACE_INVALID.error("saved results exceed 16 MiB"));
    }
    let mut header = tar::Header::new_gnu();
    header.set_size(results.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    archive.append_data(&mut header, RESULTS_PATH, results.as_slice())?;
    let bytes = archive.into_inner()?.finish()?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(WORKSPACE_INVALID.error("compressed workspace exceeds 512 MiB"));
    }
    Ok(PackedWorkspace {
        sha256: digest(&bytes),
        bytes,
        files,
    })
}

fn append_directory(
    root: &Path,
    directory: &Path,
    archive: &mut tar::Builder<GzEncoder<Vec<u8>>>,
    files: &mut Vec<WorkspaceFile>,
    size: &mut u64,
    count: &mut usize,
) -> Result<(), SendableError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        *count += 1;
        if *count > MAX_FILES {
            return Err(WORKSPACE_INVALID.error("workspace has too many files"));
        }
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        let name = relative
            .to_str()
            .ok_or_else(|| WORKSPACE_INVALID.error("file names must be UTF-8"))?;
        validate_path(relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        let archive_path = Path::new("files").join(relative);
        if metadata.is_dir() {
            archive.append_dir(&archive_path, &path)?;
            append_directory(root, &path, archive, files, size, count)?;
            continue;
        }
        let link_target = if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)?;
            validate_link(relative, &target)?;
            Some(
                target
                    .to_str()
                    .ok_or_else(|| WORKSPACE_INVALID.error("link targets must be UTF-8"))?
                    .to_string(),
            )
        } else if metadata.is_file() {
            None
        } else {
            return Err(WORKSPACE_INVALID.error("workspace contains a special file"));
        };
        let bytes = if link_target.is_none() {
            *size = size
                .checked_add(metadata.len())
                .ok_or_else(|| WORKSPACE_INVALID.error("workspace size overflow"))?;
            if *size > MAX_EXPANDED_BYTES {
                return Err(WORKSPACE_INVALID.error("workspace exceeds 2 GiB"));
            }
            fs::read(&path)?
        } else {
            Vec::new()
        };
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        if link_target.is_some() {
            archive.append_path_with_name(&path, &archive_path)?;
        } else {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(if executable { 0o700 } else { 0o600 });
            header.set_cksum();
            archive.append_data(&mut header, &archive_path, bytes.as_slice())?;
        }
        files.push(WorkspaceFile {
            path: name.replace('\\', "/"),
            size_bytes: bytes.len() as u64,
            sha256: digest(&bytes),
            executable,
            link_target,
        });
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), SendableError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(WORKSPACE_INVALID
            .error("archive path must be relative and cannot traverse directories"));
    }
    Ok(())
}

fn validate_link(path: &Path, target: &Path) -> Result<(), SendableError> {
    let mut depth = path
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(WORKSPACE_INVALID.error("symbolic link escapes the workspace")),
        }
    }
    Ok(())
}

/// Restore into a new, empty directory. Links are created last so archive entries cannot traverse them.
pub fn unpack(
    bytes: &[u8],
    root: &Path,
    expected_digest: &str,
) -> Result<BTreeMap<String, Value>, SendableError> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES || digest(bytes) != expected_digest {
        return Err(WORKSPACE_INVALID.error("archive is too large or its checksum does not match"));
    }
    if fs::read_dir(root)?.next().is_some() {
        return Err(WORKSPACE_INVALID.error("restore directory must be empty"));
    }
    let mut archive =
        tar::Archive::new(GzDecoder::new(bytes).take(MAX_EXPANDED_BYTES + 64 * 1024 * 1024));
    let mut total = 0u64;
    let mut paths = std::collections::BTreeSet::new();
    let mut links = Vec::new();
    let mut results = None;
    for (index, entry) in archive.entries()?.enumerate() {
        if index >= MAX_FILES {
            return Err(WORKSPACE_INVALID.error("archive has too many entries"));
        }
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        validate_path(&path)?;
        if !paths.insert(path.clone()) {
            return Err(WORKSPACE_INVALID.error("duplicate archive path"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| WORKSPACE_INVALID.error("archive size overflow"))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(WORKSPACE_INVALID.error("expanded archive exceeds 2 GiB"));
        }
        let kind = entry.header().entry_type();
        if path == Path::new(RESULTS_PATH) && kind.is_file() {
            if entry.size() > 16 * 1024 * 1024 {
                return Err(WORKSPACE_INVALID.error("saved results exceed 16 MiB"));
            }
            let mut json = Vec::new();
            entry.read_to_end(&mut json)?;
            results = Some(serde_json::from_slice(&json)?);
            continue;
        }
        let relative = path
            .strip_prefix("files")
            .map_err(|error| WORKSPACE_INVALID.error(error))?;
        validate_path(relative)?;
        let destination = root.join(relative);
        if kind.is_symlink() {
            let target = entry
                .link_name()?
                .ok_or_else(|| WORKSPACE_INVALID.error("link target is missing"))?
                .into_owned();
            validate_link(relative, &target)?;
            links.push((destination, target));
            continue;
        }
        if kind.is_dir() {
            fs::create_dir_all(&destination)?;
            continue;
        }
        if !kind.is_file() {
            return Err(WORKSPACE_INVALID.error("unsupported archive entry"));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &destination,
                fs::Permissions::from_mode(if entry.header().mode()? & 0o111 != 0 {
                    0o700
                } else {
                    0o600
                }),
            )?;
        }
    }
    let link_map: BTreeMap<_, _> = links
        .iter()
        .map(|(path, target)| {
            (
                path.strip_prefix(root).unwrap_or(path).to_owned(),
                target.clone(),
            )
        })
        .collect();
    for (path, target) in &link_map {
        validate_link_graph(path, target, &link_map)?;
    }
    for (destination, target) in links {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)?;
        #[cfg(not(unix))]
        {
            let _ = (destination, target);
            return Err(WORKSPACE_INVALID
                .error("symbolic link restoration is unsupported on this platform"));
        }
    }
    results.ok_or_else(|| WORKSPACE_INVALID.error("archive has no saved results"))
}

fn validate_link_graph(
    path: &Path,
    target: &Path,
    links: &BTreeMap<std::path::PathBuf, std::path::PathBuf>,
) -> Result<(), SendableError> {
    use std::collections::VecDeque;
    let joined = path.parent().unwrap_or(Path::new("")).join(target);
    let mut pending: VecDeque<_> = joined
        .components()
        .map(|part| part.as_os_str().to_owned())
        .collect();
    let mut resolved = std::path::PathBuf::new();
    let mut followed = 0;
    while let Some(part) = pending.pop_front() {
        if part == "." {
            continue;
        }
        if part == ".." {
            if !resolved.pop() {
                return Err(WORKSPACE_INVALID.error("link chain escapes workspace"));
            }
            continue;
        }
        resolved.push(&part);
        if let Some(target) = links.get(&resolved) {
            followed += 1;
            if followed > 40 {
                return Err(WORKSPACE_INVALID.error("symbolic link cycle"));
            }
            resolved.pop();
            for part in target.components().rev() {
                pending.push_front(part.as_os_str().to_owned());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod archive_tests;
