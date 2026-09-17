#[allow(unused_imports)]
use super::*;

/// Resolved database input suitable for concrete, macro-based database dispatch.
#[derive(Debug, Clone)]
pub struct DatabaseResource {
    pub(super) backend: DatabaseBackend,
    pub(super) sqlite_connection: String,
    pub(super) url: String,
}

impl DatabaseResource {
    pub fn backend(&self) -> DatabaseBackend {
        self.backend
    }

    pub fn sqlite_connection(&self) -> &str {
        &self.sqlite_connection
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}
