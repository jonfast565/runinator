mod utilities;
pub mod plugin;

use std::{collections::HashMap, fs, sync::{Arc, Mutex}};
use plugin::Plugin;
use utilities::{get_library_extension, get_library_interface};

pub fn load_libraries_from_path(path: &str, marker_function: &str) -> HashMap<String, Plugin> {
    let mut libraries = HashMap::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(&get_library_extension()) {
                if let Some(library_path) = path.to_str() {
                    if let Ok(interface) = get_library_interface(library_path, marker_function) {
                        let name = (&interface).name();
                        let plugin = Plugin {
                            interface: Arc::new(Mutex::new(interface)),
                            name: name.clone(),
                            file_name: library_path.to_string()
                        };
                        libraries.insert(name, plugin);
                    }
                }
            }
        }
    }
    libraries
}