use libloading::{Library, Symbol};

use crate::plugin::{PluginError, PluginInterface};

pub(crate) fn get_library_interface(
    library_path: &str,
    marker_function: &str,
) -> Result<Box<dyn PluginInterface>, Box<dyn std::error::Error>> {
    let lib = unsafe { Library::new(library_path) };
    if let Ok(lib) = lib {
        unsafe {
            let new_service_call: Result<
                Symbol<unsafe extern "Rust" fn() -> Box<dyn PluginInterface>>,
                _,
            > = lib.get(marker_function.as_bytes());
            if let Ok(service_interface) = new_service_call {
                let plugin_interface = service_interface();
                return Ok(plugin_interface);
            }
        }
    }

    let plugin_error = PluginError::new(
        "1".to_string(),
        format!("Could not get library interface from {}", library_path),
    );
    Err(Box::new(plugin_error))
}

pub(crate) fn get_library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}
