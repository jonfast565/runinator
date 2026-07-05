use std::path::PathBuf;

/// the workspace root, resolved from xtask's own manifest directory. xtask lives directly under
/// the workspace root, mirroring how `$PSScriptRoot` located the root for build.ps1.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate must live directly under the workspace root")
        .to_path_buf()
}

/// the cargo target directory for a given `--profile` value (`dev` maps to the `debug` dir, every
/// other profile maps to a directory of the same name, matching cargo's own convention).
pub fn target_dir(workspace_root: &std::path::Path, profile: &str) -> PathBuf {
    let dir_name = if profile == "dev" { "debug" } else { profile };
    workspace_root.join("target").join(dir_name)
}

pub fn ensure_dir(path: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}
