use std::{fmt, sync::{Arc, Mutex}};

#[derive(Clone)]
pub struct Plugin {
    pub name: String,
    pub file_name: String,
    pub interface: Arc<Mutex<Box<dyn PluginInterface>>>
}

pub trait PluginInterface : Send + Sync {
    fn name(&self) -> String;
    fn call_service(&self, name: String, args: Vec<u8>, args_length: usize);
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