pub mod cancel;
pub mod errors;
pub mod plugin;
pub mod provider;
mod utilities;

use log::{info, warn};
use plugin::Plugin;
use runinator_models::errors::SendableError;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
use utilities::get_library_extension;

const PLUGIN_LOAD_TIMEOUT: Duration = Duration::from_secs(5);

pub fn load_libraries_from_path(path: &str) -> Result<HashMap<String, Plugin>, SendableError> {
    let path_dir = PathBuf::from(path);
    let canonical_dir: PathBuf = fs::canonicalize(path_dir)?;
    info!("Loading libraries from {}", canonical_dir.display());
    let mut libraries = HashMap::new();
    let extension = get_library_extension();
    for entry in fs::read_dir(canonical_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            match load_plugin_with_timeout(path.clone(), PLUGIN_LOAD_TIMEOUT) {
                Ok(plugin) => {
                    libraries.insert(plugin.name.clone(), plugin);
                }
                Err(err) => {
                    warn!("Skipping plugin {}: {}", path.display(), err);
                }
            }
        }
    }
    Ok(libraries)
}

fn load_plugin_with_timeout(path: PathBuf, timeout: Duration) -> Result<Plugin, SendableError> {
    let (tx, rx) = mpsc::channel();
    let worker_path = path.clone();
    std::thread::spawn(move || {
        let _ = tx.send(Plugin::new(&worker_path));
    });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(crate::errors::LOAD_FAILED.error(format!(
            "loading {} exceeded {}s",
            display_path(&path),
            timeout.as_secs()
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(crate::errors::LOAD_FAILED.error(
            format!("loading {} ended without a result", display_path(&path)),
        )),
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn print_libs(libs_list: &HashMap<String, Plugin>) {
    info!("{} libraries loaded", libs_list.len());
    for (i, p) in libs_list.iter() {
        info!("Library {} <- `{}`", i, p.file_name.display())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
