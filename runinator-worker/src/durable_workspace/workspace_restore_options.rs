#[allow(unused_imports)]
use super::*;

pub(super) struct WorkspaceRestoreOptions {
    pub(super) root: std::path::PathBuf,
    pub(super) materialize_files: bool,
    pub(super) load_results: bool,
}
