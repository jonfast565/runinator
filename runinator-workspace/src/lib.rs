//! Workspace results, materialization, and native transfers over immutable paged storage.
use runinator_models::errors::{SendableError, WORKSPACE_INVALID};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};
pub mod errors;
pub mod native;
mod results;
pub mod revision;
pub use results::resolve_results;
pub use runinator_workspace_storage as storage;

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
        let Some(target) = links.get(&resolved) else {
            continue;
        };

        followed += 1;
        if followed > 40 {
            return Err(WORKSPACE_INVALID.error("symbolic link cycle"));
        }
        resolved.pop();
        for part in target.components().rev() {
            pending.push_front(part.as_os_str().to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod path_tests;

mod reconcile;

pub mod filesystem;
