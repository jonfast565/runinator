mod utilities;

use std::{collections::HashMap, fs};
use utilities::{get_library_extension, get_library_name};

pub fn load_libraries_from_path(path: &str, marker_function: &str) -> HashMap<String, String> {
    let mut libraries = HashMap::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(&get_library_extension()) {
                if let Some(library_path) = path.to_str() {
                    if let Some(name) = get_library_name(library_path, &marker_function) {
                        libraries.insert(name, library_path.to_string());
                    }
                }
            }
        }
    }
    libraries
}