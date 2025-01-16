mod utilities;
mod errors;
pub mod plugin;
pub mod provider;

use std::{collections::HashMap, fs, path::PathBuf};
use log::info;
use plugin::Plugin;
use utilities::get_library_extension;

pub fn load_libraries_from_path(path: &str) -> Result<HashMap<String, Plugin>, Box<dyn std::error::Error>> {
    let path_dir = PathBuf::from(path);
    let canonical_dir = fs::canonicalize(path_dir).expect("path not valid");
    info!("Loading libraries from {}", canonical_dir.as_os_str().to_str().unwrap());
    let mut libraries = HashMap::new();
    let extension = get_library_extension();
    if let Ok(entries) = fs::read_dir(canonical_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
                let plugin = Plugin::new(&path)?;
                libraries.insert(plugin.name.clone(), plugin);
            }
        }
    }
    Ok(libraries)
}

pub fn print_libs(libs_list: &HashMap<String, Plugin>) {
    info!("{} libraries loaded", libs_list.len());
    for (i, p) in libs_list.iter() {
        info!("Library {} <- `{}`", i, p.file_name.as_os_str().to_str().unwrap().to_string())
    }
}