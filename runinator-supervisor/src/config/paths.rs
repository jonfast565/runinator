#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub struct Paths {
    pub config_path: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub pid_file: PathBuf,
    pub stop_file: PathBuf,
    pub state_file: PathBuf,
    pub control_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub supervisor_log: PathBuf,
}
