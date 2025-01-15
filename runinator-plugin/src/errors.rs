use std::fmt;

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