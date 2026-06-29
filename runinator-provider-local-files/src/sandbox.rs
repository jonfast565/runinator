use std::path::{Component, Path, PathBuf};

use runinator_models::errors::SendableError;

use crate::errors::{IO, PATH_OUTSIDE_ROOT, ROOT_NOT_CONFIGURED};

/// env var naming the directory this worker is confined to. set by the desktop host, never by the
/// server, so a workflow can never widen the sandbox.
pub(crate) const ROOT_ENV: &str = "RUNINATOR_LOCAL_FILES_ROOT";
/// env var that, when truthy, permits the mutating actions (`write_file`, `delete`).
pub(crate) const ALLOW_WRITE_ENV: &str = "RUNINATOR_LOCAL_FILES_ALLOW_WRITE";

/// locality marker stamped on every result and artifact this provider produces. it distinguishes
/// files that live on the user's machine (reachable only while this desktop worker is connected)
/// from cloud-stored artifacts the web service can stream directly.
pub(crate) const LOCATION_LOCAL: &str = "local";

/// the canonical, existing sandbox root. errors if unset or missing so the provider fails closed.
pub(crate) fn root() -> Result<PathBuf, SendableError> {
    let raw = std::env::var(ROOT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ROOT_NOT_CONFIGURED.error(ROOT_ENV))?;
    std::fs::canonicalize(&raw).map_err(|err| ROOT_NOT_CONFIGURED.error(format!("{raw}: {err}")))
}

/// whether the mutating actions are permitted on this worker.
pub(crate) fn writes_allowed() -> bool {
    std::env::var(ALLOW_WRITE_ENV)
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

// lexically join `rel` under `root`, rejecting absolute paths and any `..` escape before touching
// the filesystem. the result is guaranteed to be lexically within `root`; symlink escapes are caught
// later by canonicalizing the resolved path.
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, SendableError> {
    let rel_path = Path::new(rel);
    for component in rel_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // RootDir/Prefix (absolute) or ParentDir (`..`) would escape the sandbox.
            _ => return Err(PATH_OUTSIDE_ROOT.error(rel)),
        }
    }
    Ok(root.join(rel_path))
}

// confirm a canonicalized path is contained by the canonical root, defeating symlink escapes.
fn ensure_within(root: &Path, resolved: &Path, rel: &str) -> Result<(), SendableError> {
    if resolved.starts_with(root) {
        Ok(())
    } else {
        Err(PATH_OUTSIDE_ROOT.error(rel))
    }
}

/// resolve `rel` to an existing path strictly inside `root`. used by read/list/stat/delete.
pub(crate) fn resolve_existing(root: &Path, rel: &str) -> Result<PathBuf, SendableError> {
    let candidate = safe_join(root, rel)?;
    let resolved = std::fs::canonicalize(&candidate).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            crate::errors::NOT_FOUND.error(rel)
        } else {
            IO.error(format!("{rel}: {err}"))
        }
    })?;
    ensure_within(root, &resolved, rel)?;
    Ok(resolved)
}

/// resolve `rel` to a (possibly not-yet-existing) write target strictly inside `root`, creating its
/// parent directory if needed. used by write_file.
pub(crate) fn resolve_for_write(root: &Path, rel: &str) -> Result<PathBuf, SendableError> {
    let candidate = safe_join(root, rel)?;
    let parent = candidate
        .parent()
        .ok_or_else(|| PATH_OUTSIDE_ROOT.error(rel))?;
    std::fs::create_dir_all(parent).map_err(|err| IO.error(format!("{rel}: {err}")))?;
    // canonicalize the now-existing parent and confirm it (and thus the target) stays in root.
    let resolved_parent =
        std::fs::canonicalize(parent).map_err(|err| IO.error(format!("{rel}: {err}")))?;
    ensure_within(root, &resolved_parent, rel)?;
    let file_name = candidate
        .file_name()
        .ok_or_else(|| PATH_OUTSIDE_ROOT.error(rel))?;
    Ok(resolved_parent.join(file_name))
}

/// resolve `rel` lexically for a stat probe without requiring existence. returns `None` when the
/// (sandbox-valid) target does not exist, so `stat` can report `exists: false` gracefully.
pub(crate) fn resolve_optional(root: &Path, rel: &str) -> Result<Option<PathBuf>, SendableError> {
    let candidate = safe_join(root, rel)?;
    match std::fs::canonicalize(&candidate) {
        Ok(resolved) => {
            ensure_within(root, &resolved, rel)?;
            Ok(Some(resolved))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(IO.error(format!("{rel}: {err}"))),
    }
}
