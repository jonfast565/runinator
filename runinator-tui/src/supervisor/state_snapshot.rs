#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub supervisor_pid: u32,
    pub config_path: String,
    pub started_at: String,
    pub updated_at: String,
    pub processes: Vec<ProcessSnapshot>,
}
