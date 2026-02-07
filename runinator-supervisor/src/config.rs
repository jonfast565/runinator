use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::types::DynError;

#[derive(Debug, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default = "default_restart_delay_ms")]
    pub restart_delay_ms: u64,
    #[serde(default)]
    pub processes: Vec<ProcessConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub restart_on_failure: bool,
    #[serde(default = "default_max_restarts_per_minute")]
    pub max_restarts_per_minute: u32,
}

#[derive(Debug)]
pub struct Paths {
    pub config_path: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub pid_file: PathBuf,
    pub stop_file: PathBuf,
    pub state_file: PathBuf,
    pub logs_dir: PathBuf,
    pub supervisor_log: PathBuf,
}

pub fn load_config(path: &Path) -> Result<(SupervisorConfig, Paths), DynError> {
    let cwd = env::current_dir()?;
    let config_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let config_path = config_path.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Unable to resolve config path {}: {err}", config_path.display()),
        )
    })?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| io::Error::other("Config path has no parent directory"))?
        .to_path_buf();

    let data = fs::read_to_string(&config_path)?;
    let config: SupervisorConfig = serde_json::from_str(&data).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid config JSON in {}: {err}", config_path.display()),
        )
    })?;

    if config.processes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Config has no processes").into());
    }

    let state_dir = resolve_path(&config_dir, Path::new(&config.state_dir));
    let paths = Paths {
        config_path,
        config_dir,
        pid_file: state_dir.join("supervisor.pid"),
        stop_file: state_dir.join("stop"),
        state_file: state_dir.join("state.json"),
        logs_dir: state_dir.join("logs"),
        supervisor_log: state_dir.join("supervisor.log"),
        state_dir,
    };

    Ok((config, paths))
}

pub fn resolve_path(base_dir: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    }
}

fn default_true() -> bool {
    true
}

fn default_state_dir() -> String {
    ".runinator-supervisor".to_string()
}

fn default_shutdown_timeout_secs() -> u64 {
    10
}

fn default_restart_delay_ms() -> u64 {
    2000
}

fn default_max_restarts_per_minute() -> u32 {
    10
}
