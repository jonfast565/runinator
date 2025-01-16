use std::fmt;

pub type SendableError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
pub struct RuntimeError {
    code: String,
    message: String
}

impl RuntimeError {
    pub fn new(code: String, message: String) -> Self {
        Self {
            code,
            message
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}