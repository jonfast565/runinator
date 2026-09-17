#[allow(unused_imports)]
use super::*;

/// Explicit database selection parsed by a service's existing CLI.
#[derive(Debug, Clone)]
pub struct DatabaseRequest {
    pub backend: DatabaseBackend,
    pub sqlite_path: Option<PathBuf>,
    pub database_url: Option<String>,
}
