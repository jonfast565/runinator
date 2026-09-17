#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct LogFile {
    pub(super) path: PathBuf,
    pub(super) modified: SystemTime,
    pub(super) bytes: u64,
    pub(super) protected: bool,
}
