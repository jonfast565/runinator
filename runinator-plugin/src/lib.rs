mod utilities;
pub mod plugin;

use std::{collections::HashMap, fs, path::PathBuf, sync::{Arc, Mutex}};
use libloading::{Library, Symbol};
use log::info;
use plugin::{Plugin, PluginInterface};
use utilities::get_library_extension;

type PluginInterfaceFn = unsafe extern "Rust" fn() -> Box<dyn PluginInterface>;

pub fn load_libraries_from_path(path: &str, marker_function: &str) -> HashMap<String, Plugin> {
    let path_dir = PathBuf::from(path);
    let canonical_dir = fs::canonicalize(path_dir).expect("path not valid");
    info!("Loading libraries from {} using marker function {}", canonical_dir.as_os_str().to_str().unwrap(), marker_function);
    let mut libraries = HashMap::new();
    let extension = get_library_extension();
    if let Ok(entries) = fs::read_dir(canonical_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
                if let Some(library_path) = path.to_str() {
                    info!("Found library {}", path.as_os_str().to_str().unwrap());
                    let null_termd_marker = marker_function.to_string() + "\0";
                    let marker_function_bytes = null_termd_marker.as_bytes();
                    let lib = unsafe { Library::new(library_path) };
                    if let Ok(lib) = lib {
                        let new_service_call: Result<Symbol<PluginInterfaceFn>, _> = unsafe { lib.get(marker_function_bytes) };
                        if let Ok(service_interface) = new_service_call {
                            let plugin_interface: Box<dyn PluginInterface> = unsafe { service_interface() };
                            let name = plugin_interface.name();
                            let plugin = Plugin {
                                interface: Arc::new(Mutex::new(plugin_interface)),
                                name: name.clone(),
                                file_name: library_path.to_string()
                            };
                            libraries.insert(name, plugin);
                        }
                    }
                }
            }
        }
    }
    libraries
}

pub fn print_libs(libs_list: &HashMap<String, Plugin>) {
    info!("{} libraries loaded", libs_list.len());
    for (i, p) in libs_list.iter() {
        info!("Library {} <- `{}`", i, p.file_name)
    }
}