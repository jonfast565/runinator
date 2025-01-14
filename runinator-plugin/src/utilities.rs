use libloading::{Library, Symbol};
use log::info;

use crate::plugin::{PluginError, PluginInterface};

type PluginInterfaceFn = unsafe extern "Rust" fn() -> Box<dyn PluginInterface>;

pub(crate) fn get_library_interface(
    library_path: &str,
    marker_function: &str,
) -> Result<Box<dyn PluginInterface>, Box<dyn std::error::Error>> {
    info!("Loading library {}", library_path);
    let lib = unsafe { Library::new(library_path) };
    let marker_function_bytes = marker_function.as_bytes();
    if let Ok(lib) = lib {
        unsafe {
            let new_service_call: Result<Symbol<PluginInterfaceFn>, _> = lib.get(marker_function_bytes);
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
