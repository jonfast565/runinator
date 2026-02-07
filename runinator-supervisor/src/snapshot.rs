use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::types::DynError;

#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub supervisor_pid: u32,
    pub config_path: String,
    pub started_at: String,
    pub updated_at: String,
    pub processes: Vec<ProcessSnapshot>,
}

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

pub fn write_snapshot(path: &Path, snapshot: &StateSnapshot) -> Result<(), DynError> {
    let temp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(snapshot)?;
    fs::write(&temp, body)?;
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn read_snapshot(path: &Path) -> Result<StateSnapshot, DynError> {
    let data = fs::read_to_string(path)?;
    let snapshot: StateSnapshot = serde_json::from_str(&data)?;
    Ok(snapshot)
}
