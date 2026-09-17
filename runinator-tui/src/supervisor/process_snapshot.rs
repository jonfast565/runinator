#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub uptime_seconds: Option<u64>,
    pub last_exit_code: Option<i32>,
    pub last_error: Option<String>,
    pub started_at: Option<String>,
    pub command: String,
    pub cwd: String,
    pub log_file: String,
}
