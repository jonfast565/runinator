use std::{fmt, path::PathBuf};
use libloading::{Library, Symbol};

type PluginInterfaceFn = unsafe extern "Rust" fn() -> Box<dyn PluginInterface>;

pub trait PluginInterface: Send + Sync {
    fn name(&self) -> String;
    fn call_service(&self, name: String, args: String);
}

#[derive(Clone)]
pub struct Plugin {
    pub name: String,
    pub file_name: PathBuf,
    pub marker_function: String
}

impl Plugin {
    pub fn new(path: &PathBuf, marker_function: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let name = Plugin::init_plugin_name(path, marker_function)?;
        Ok(Plugin {
            name,
            file_name: path.clone(),
            marker_function: marker_function.to_string()
        })
    }

    pub fn init_plugin_name(path: &PathBuf, marker_function: &str) -> Result<String, Box<dyn std::error::Error>> {
        unsafe {
            let null_termd_marker = marker_function.to_string() + "\0";
            let marker_function_bytes = null_termd_marker.as_bytes();
            let lib = Library::new(path)?;
            let symbol: Symbol<PluginInterfaceFn> = lib.get(marker_function_bytes)?;
            let plugin_interface: Box<dyn PluginInterface> = (symbol)();
            let name = plugin_interface.name();
            Ok(name)
        }
    }

    pub fn plugin_service_call(&self, name: String, args: String) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let null_termd_marker = self.marker_function.to_string() + "\0";
            let marker_function_bytes = null_termd_marker.as_bytes();
            let lib = Library::new(self.file_name.clone())?;
            let symbol: Symbol<PluginInterfaceFn> = lib.get(marker_function_bytes)?;
            let plugin_interface: Box<dyn PluginInterface> = (symbol)();
            plugin_interface.call_service(name.clone(), args.clone());
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct PluginError {
    code: String,
    message: String
}

impl PluginError {
    pub fn new(code: String, message: String) -> Self {
        Self {
            code,
            message
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}